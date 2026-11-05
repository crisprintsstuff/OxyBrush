//! Python plugin and scripting system for OxyBrush.
//!
//! Scans for `.py` plugins in the user plugins directory and allows running
//! them against the running OxyBrush instance via the local MCP agent bridge.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

/// Metadata and path of a discovered Python plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginInfo {
    /// User-visible plugin display name.
    pub name: String,
    /// Optional short description of what the plugin does.
    pub description: String,
    /// Absolute or relative path to the `.py` file.
    pub path: PathBuf,
}

/// A running background plugin process.
pub struct PluginProcess {
    /// Display name of the running plugin.
    pub name: String,
    /// Channel yielding the completion result (`Ok(stdout)` or `Err(stderr)`).
    pub rx: Receiver<Result<String, String>>,
}

/// Path to the user plugins directory (`<data_local_dir>/oxybrush/plugins`).
pub fn plugins_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("oxybrush").join("plugins"))
}

/// Open the given folder in the system file manager.
pub fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    #[cfg(target_os = "windows")]
    let _ = Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(path).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = Command::new("xdg-open").arg(path).spawn();
}

/// Parse metadata from the top lines of a `.py` script.
///
/// Recognizes lines such as `# Name: My Plugin` and `# Description: Does stuff`
/// or inside docstrings `""" Name: ... """`.
pub fn parse_plugin_metadata(path: &Path) -> Option<PluginInfo> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut name = None;
    let mut description = None;

    // Scan up to the first 40 lines
    for line in content.lines().take(40) {
        let trimmed = line.trim().trim_start_matches(['#', '"', '\'']).trim();
        if let Some(rest) = strip_prefix_ignore_case(trimmed, "Name:") {
            if name.is_none() {
                name = Some(rest.trim().trim_end_matches(['"', '\'']).trim().to_string());
            }
        } else if let Some(rest) = strip_prefix_ignore_case(trimmed, "Description:")
            && description.is_none()
        {
            description = Some(rest.trim().trim_end_matches(['"', '\'']).trim().to_string());
        }
    }

    let final_name = name.filter(|n| !n.is_empty()).unwrap_or_else(|| {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled");
        format_fallback_name(stem)
    });

    Some(PluginInfo { name: final_name, description: description.unwrap_or_default(), path: path.to_path_buf() })
}

fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

fn format_fallback_name(stem: &str) -> String {
    let mut words = Vec::new();
    for part in stem.split(['_', '-']) {
        if part.is_empty() {
            continue;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            let cap = first.to_uppercase().collect::<String>() + chars.as_str();
            words.push(cap);
        }
    }
    if words.is_empty() { "Untitled Plugin".into() } else { words.join(" ") }
}

/// Discover all `.py` plugin files from the default user directory and workspace examples.
pub fn discover_plugins() -> Vec<PluginInfo> {
    let mut plugins = Vec::new();

    // 1. User plugins directory
    if let Some(dir) = plugins_dir() {
        ensure_plugins_dir_initialized(&dir);
        scan_dir(&dir, &mut plugins);
    }

    // 2. Examples plugins directory if running in workspace or development
    let dev_paths = [PathBuf::from("examples/plugins"), PathBuf::from("../examples/plugins")];
    for dev_path in dev_paths {
        if dev_path.is_dir() {
            scan_dir(&dev_path, &mut plugins);
        }
    }

    plugins.sort_by_key(|a| a.name.to_lowercase());
    plugins
}

fn ensure_plugins_dir_initialized(dir: &Path) {
    if !dir.exists() && std::fs::create_dir_all(dir).is_ok() {
        let readme = dir.join("README.txt");
        let _ = std::fs::write(
            readme,
            "OxyBrush Plugins Directory\n\
             ==========================\n\n\
             Place Python (.py) scripts in this directory. They will appear\n\
             in OxyBrush under the 'Plugins' menu.\n\n\
             Metadata header format:\n\
             # Name: My Tool Name\n\
             # Description: Brief explanation of the tool\n\n\
             Example usage:\n\
             import oxybrush\n\
             app = oxybrush.attach()\n\
             app.select_brush('Clay')\n",
        );
    }
}

fn scan_dir(dir: &Path, out: &mut Vec<PluginInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("py")) == Some(true)
        {
            // Avoid duplicates
            if out.iter().any(|p| p.path == path) {
                continue;
            }
            if let Some(info) = parse_plugin_metadata(&path) {
                out.push(info);
            }
        }
    }
}

/// Find a Python 3 executable on the system.
pub fn find_python() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OXYBRUSH_PYTHON") {
        let path = PathBuf::from(p);
        if test_python(&path) {
            return Some(path);
        }
    }
    if let Some(p) = std::env::var_os("PYTHON") {
        let path = PathBuf::from(p);
        if test_python(&path) {
            return Some(path);
        }
    }

    #[cfg(windows)]
    let candidates = ["python3.exe", "python.exe", "py.exe"];
    #[cfg(not(windows))]
    let candidates = ["python3", "python"];

    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if test_python(&path) {
            return Some(path);
        }
    }

    None
}

fn test_python(path: &Path) -> bool {
    Command::new(path).arg("--version").output().map(|out| out.status.success()).unwrap_or(false)
}

/// Locate the `python/` SDK directory containing `oxybrush`.
pub fn python_sdk_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OXYBRUSH_PYTHONPATH") {
        let path = PathBuf::from(p);
        if path.is_dir() {
            return Some(path);
        }
    }

    // Try relative to current working directory
    let cwd_path = PathBuf::from("python");
    if cwd_path.join("oxybrush").join("__init__.py").is_file() {
        return std::fs::canonicalize(&cwd_path).ok().or(Some(cwd_path));
    }

    // Try CARGO_MANIFEST_DIR if running tests
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let manifest_path = PathBuf::from(manifest);
        if let Some(root) = manifest_path.parent().and_then(|p| p.parent()) {
            let candidate = root.join("python");
            if candidate.join("oxybrush").join("__init__.py").is_file() {
                return std::fs::canonicalize(&candidate).ok().or(Some(candidate));
            }
        }
    }

    // Try walking up from the current executable
    if let Ok(exe) = std::env::current_exe() {
        let mut curr = exe.as_path();
        for _ in 0..6 {
            if let Some(parent) = curr.parent() {
                let candidate = parent.join("python");
                if candidate.join("oxybrush").join("__init__.py").is_file() {
                    return std::fs::canonicalize(&candidate).ok().or(Some(candidate));
                }
                curr = parent;
            } else {
                break;
            }
        }
    }

    // Try local data dir
    if let Some(data_dir) = dirs::data_local_dir() {
        let candidate = data_dir.join("oxybrush").join("python");
        if candidate.join("oxybrush").join("__init__.py").is_file() {
            return Some(candidate);
        }
    }

    None
}

/// Run a Python plugin script against the given OxyBrush MCP endpoint and token.
pub fn run_plugin(
    script_path: &Path,
    endpoint: &str,
    token: &str,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> Result<PluginProcess, String> {
    let python_bin = find_python().ok_or_else(|| {
        "Python 3 was not found. Please install Python 3 and ensure it is in your PATH or set OXYBRUSH_PYTHON."
            .to_string()
    })?;

    let plugin_info = parse_plugin_metadata(script_path);
    let name = plugin_info.map(|p| p.name).unwrap_or_else(|| "Python Plugin".into());

    let (tx, rx) = channel();

    let mut cmd = Command::new(python_bin);
    cmd.arg(script_path);
    cmd.env("OXYBRUSH_MCP_ENDPOINT", endpoint);
    cmd.env("OXYBRUSH_MCP_TOKEN", token);

    if let Some(sdk_path) = python_sdk_dir() {
        let current_pythonpath = std::env::var("PYTHONPATH").unwrap_or_default();
        let new_pythonpath = if current_pythonpath.is_empty() {
            sdk_path.to_string_lossy().into_owned()
        } else {
            #[cfg(windows)]
            let sep = ";";
            #[cfg(not(windows))]
            let sep = ":";
            format!("{}{sep}{current_pythonpath}", sdk_path.display())
        };
        cmd.env("PYTHONPATH", new_pythonpath);
    }

    std::thread::Builder::new()
        .name(format!("plugin-{}", script_path.display()))
        .spawn(move || {
            let res = match cmd.output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    if output.status.success() {
                        Ok(stdout)
                    } else {
                        let msg = if !stderr.is_empty() {
                            stderr
                        } else if !stdout.is_empty() {
                            stdout
                        } else {
                            format!("Exited with code {:?}", output.status.code())
                        };
                        Err(msg)
                    }
                }
                Err(e) => Err(format!("Failed to execute Python process: {e}")),
            };

            let _ = tx.send(res);
            wake();
        })
        .map_err(|e| format!("Failed to spawn plugin worker thread: {e}"))?;

    Ok(PluginProcess { name, rx })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_fallback_name() {
        assert_eq!(format_fallback_name("spiral_generator"), "Spiral Generator");
        assert_eq!(format_fallback_name("mesh-stats"), "Mesh Stats");
        assert_eq!(format_fallback_name("test"), "Test");
        assert_eq!(format_fallback_name(""), "Untitled Plugin");
    }

    #[test]
    fn test_parse_metadata() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_plugin_sample.py");
        std::fs::write(
            &test_file,
            "\"\"\"\n# Name: Cool Curve Generator\n# Description: Creates awesome geometry\n\"\"\"\nimport math\n",
        )
        .unwrap();

        let info = parse_plugin_metadata(&test_file).expect("should parse");
        assert_eq!(info.name, "Cool Curve Generator");
        assert_eq!(info.description, "Creates awesome geometry");
        assert_eq!(info.path, test_file);

        let _ = std::fs::remove_file(&test_file);
    }

    #[test]
    fn test_run_python_script_simple() {
        if find_python().is_none() {
            return;
        }
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_run_plugin.py");
        std::fs::write(&test_file, "import sys\nprint('Hello from OxyBrush plugin!')\nsys.exit(0)\n").unwrap();

        let wake = Arc::new(|| {});
        let proc = run_plugin(&test_file, "http://127.0.0.1:41799/mcp", "test_token", wake).expect("should spawn");
        let res = proc.rx.recv_timeout(std::time::Duration::from_secs(5)).expect("reply");
        assert_eq!(res.unwrap(), "Hello from OxyBrush plugin!");

        let _ = std::fs::remove_file(&test_file);
    }

    #[test]
    fn test_python_sdk_e2e_communication() {
        if find_python().is_none() {
            return;
        }
        let (wake_tx, wake_rx) = std::sync::mpsc::channel();
        let wake = Arc::new(move || {
            let _ = wake_tx.send(());
        });
        let mut inbox = match crate::agent::Inbox::listen(wake.clone()) {
            Ok(ib) => ib,
            Err(_) => return, // port busy or network restricted
        };
        let endpoint = inbox.endpoint.clone();
        let token = inbox.token();

        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_sdk_e2e.py");
        std::fs::write(
            &test_file,
            "import oxybrush\n\
             app = oxybrush.attach()\n\
             pieces = app.list_pieces()\n\
             assert len(pieces) >= 1\n\
             stats = app.get_mesh_stats()\n\
             assert stats.piece_count >= 1\n\
             assert stats.vertex_count > 0\n\
             brush_msg = app.select_brush('Clay')\n\
             assert 'Clay' in brush_msg\n\
             print('E2E SUCCESS')\n",
        )
        .unwrap();

        let proc = run_plugin(&test_file, &endpoint, &token, wake).expect("spawn plugin");
        let mut ed = crate::editor::Editor::default();
        let start = std::time::Instant::now();

        let mut res = None;
        while start.elapsed() < std::time::Duration::from_secs(30) {
            let _ = inbox.poll(&mut ed);
            if let Ok(r) = proc.rx.try_recv() {
                res = Some(r);
                break;
            }
            let _ = wake_rx.recv_timeout(std::time::Duration::from_millis(50));
        }

        let _ = std::fs::remove_file(&test_file);
        let output = res.expect("process should complete").expect("plugin should succeed");
        assert_eq!(output, "E2E SUCCESS");
    }

    #[test]
    fn test_example_plugin_mesh_stats() {
        if find_python().is_none() {
            return;
        }
        let (wake_tx, wake_rx) = std::sync::mpsc::channel();
        let wake = Arc::new(move || {
            let _ = wake_tx.send(());
        });
        let mut inbox = match crate::agent::Inbox::listen(wake.clone()) {
            Ok(ib) => ib,
            Err(_) => return,
        };
        let endpoint = inbox.endpoint.clone();
        let token = inbox.token();

        let plugin_path = PathBuf::from("examples/plugins/mesh_stats.py");
        if !plugin_path.is_file() {
            return;
        }

        let proc = run_plugin(&plugin_path, &endpoint, &token, wake).expect("spawn mesh_stats");
        let mut ed = crate::editor::Editor::default();
        let start = std::time::Instant::now();

        let mut res = None;
        while start.elapsed() < std::time::Duration::from_secs(30) {
            let _ = inbox.poll(&mut ed);
            if let Ok(r) = proc.rx.try_recv() {
                res = Some(r);
                break;
            }
            let _ = wake_rx.recv_timeout(std::time::Duration::from_millis(50));
        }

        let output = res.expect("process should complete").expect("mesh_stats should succeed");
        assert!(output.contains("OxyBrush Scene Inspector"), "{output}");
        assert!(output.contains("Total pieces: 1"), "{output}");
    }

    #[test]
    fn test_example_plugin_mesh_stats_reporter() {
        if find_python().is_none() {
            return;
        }
        let (wake_tx, wake_rx) = std::sync::mpsc::channel();
        let wake = Arc::new(move || {
            let _ = wake_tx.send(());
        });
        let mut inbox = match crate::agent::Inbox::listen(wake.clone()) {
            Ok(ib) => ib,
            Err(_) => return,
        };
        let endpoint = inbox.endpoint.clone();
        let token = inbox.token();

        let plugin_path = PathBuf::from("examples/plugins/mesh_stats_reporter.py");
        if !plugin_path.is_file() {
            return;
        }

        let proc = run_plugin(&plugin_path, &endpoint, &token, wake).expect("spawn mesh_stats_reporter");
        let mut ed = crate::editor::Editor::default();
        let start = std::time::Instant::now();

        let mut res = None;
        while start.elapsed() < std::time::Duration::from_secs(30) {
            let _ = inbox.poll(&mut ed);
            if let Ok(r) = proc.rx.try_recv() {
                res = Some(r);
                break;
            }
            let _ = wake_rx.recv_timeout(std::time::Duration::from_millis(50));
        }

        let output = res.expect("process should complete").expect("mesh_stats_reporter should succeed");
        assert!(output.contains("piece(s)"), "{output}");
        assert!(output.contains("tris"), "{output}");
    }
}
