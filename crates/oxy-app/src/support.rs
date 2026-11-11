//! Tester support: a log file, panic logging, system info and issue links.
//!
//! Release builds on Windows have no console, so without a log file there
//! is nothing to attach to a bug report. Logs go to the per-user data
//! directory (`%LOCALAPPDATA%\OxyBrush\logs` on Windows,
//! `~/.local/share/oxybrush/logs` on Linux, inside `~/.var/app/…` for the
//! Flatpak). Each launch starts `oxybrush.log`; the previous run is kept as
//! `oxybrush.previous.log` so a crash log survives the restart.

use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;

pub const REPO_URL: &str = "https://github.com/crisprintsstuff/OxyBrush";

pub fn log_dir() -> Option<PathBuf> {
    let app = if cfg!(windows) { "OxyBrush" } else { "oxybrush" };
    Some(dirs::data_local_dir()?.join(app).join("logs"))
}

/// Writes every log line to stderr and (if available) the log file.
struct Tee {
    file: Option<File>,
}

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(buf);
        if let Some(f) = &mut self.file {
            // Logging must never take the app down: ignore file errors.
            let _ = f.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(f) = &mut self.file {
            let _ = f.flush();
        }
        Ok(())
    }
}

/// Set up logging to stderr + log file and log panics. Returns the log file path.
pub fn init_logging() -> Option<PathBuf> {
    let path = log_dir().and_then(|dir| {
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("oxybrush.log");
        if path.exists() {
            let _ = std::fs::rename(&path, dir.join("oxybrush.previous.log"));
        }
        Some(path)
    });
    let file = path.as_ref().and_then(|p| File::create(p).ok());
    let path = if file.is_some() { path } else { None };

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"),
    )
    .target(env_logger::Target::Pipe(Box::new(Tee { file })))
    .init();

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("PANIC: {info}\n{}", std::backtrace::Backtrace::force_capture());
        log::logger().flush();
        default_hook(info);
    }));

    log::info!("OxyBrush {} on {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
    if let Some(p) = &path {
        log::info!("log file: {}", p.display());
    }
    path
}

/// What a bug report needs to know about this machine.
#[derive(Debug, Clone, Default)]
pub struct SystemInfo {
    pub gpu: Option<String>,
    pub gpu_backend: Option<String>,
    pub gpu_driver: Option<String>,
    pub exact_face_colors: Option<bool>,
    pub log_path: Option<PathBuf>,
}

impl SystemInfo {
    pub fn os() -> String {
        let flatpak = std::env::var_os("FLATPAK_ID").is_some();
        format!("{} {}{}", std::env::consts::OS, std::env::consts::ARCH, if flatpak { " (Flatpak)" } else { "" })
    }

    pub fn gpu_summary(&self) -> String {
        match (&self.gpu, &self.gpu_backend) {
            (Some(g), Some(b)) => format!("{g} ({b})"),
            (Some(g), None) => g.clone(),
            _ => "unknown".into(),
        }
    }

    /// Plain-text block to paste into an issue.
    pub fn text(&self) -> String {
        let mut s =
            format!("OxyBrush: {}\nOS: {}\nGPU: {}\n", env!("CARGO_PKG_VERSION"), Self::os(), self.gpu_summary());
        if let Some(d) = &self.gpu_driver {
            s.push_str(&format!("GPU driver: {d}\n"));
        }
        if let Some(e) = self.exact_face_colors {
            s.push_str(&format!("Exact face-group colors: {e}\n"));
        }
        if let Some(p) = &self.log_path {
            s.push_str(&format!("Log file: {}\n", p.display()));
        }
        s
    }

    /// New-issue URL for the bug report form, with version/OS/GPU pre-filled.
    pub fn bug_report_url(&self) -> String {
        format!(
            "{REPO_URL}/issues/new?template=bug_report.yml&version={}&os={}&gpu={}",
            encode(env!("CARGO_PKG_VERSION")),
            encode(&Self::os()),
            encode(&self.gpu_summary())
        )
    }

    pub fn feedback_url(&self) -> String {
        format!("{REPO_URL}/issues/new?template=feedback.yml&version={}", encode(env!("CARGO_PKG_VERSION")))
    }
}

/// Percent-encode a query-string value (RFC 3986 unreserved characters kept).
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Load a PNG as a brush alpha (luminance; transparent pixels count as black).
pub fn load_alpha_png(path: &std::path::Path) -> Result<oxy_sculpt::Alpha, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = png::Decoder::new_with_limits(io::BufReader::new(file), png::Limits { bytes: 256 << 20 });
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width, info.height);
    let px = &buf[..info.buffer_size()];
    let luma: Vec<u8> = match info.color_type {
        png::ColorType::Grayscale => px.to_vec(),
        png::ColorType::GrayscaleAlpha => {
            px.as_chunks::<2>().0.iter().map(|p| ((p[0] as u16 * p[1] as u16) / 255) as u8).collect()
        }
        png::ColorType::Rgb => px.as_chunks::<3>().0.iter().map(|p| luminance(p)).collect(),
        png::ColorType::Rgba => {
            px.as_chunks::<4>().0.iter().map(|p| ((luminance(p) as u16 * p[3] as u16) / 255) as u8).collect()
        }
        other => return Err(format!("unsupported PNG color type {other:?}")),
    };
    let name = path.file_stem().map_or("Alpha".into(), |s| s.to_string_lossy().into_owned());
    oxy_sculpt::Alpha::from_luma(name, w, h, &luma).ok_or_else(|| "empty image".to_string())
}

/// Write `alpha` as a 16-bit grayscale PNG (white = 1), for editing in a 2D
/// app; `load_alpha_png` reads it back. Written to a temporary file next to
/// `path` and renamed over it, so a failed write never leaves half a file.
pub fn write_alpha_png(alpha: &oxy_sculpt::Alpha, path: &std::path::Path) -> Result<(), String> {
    let n = alpha.size;
    let mut px = Vec::with_capacity(alpha.data.len() * 2);
    for v in &alpha.data {
        px.extend_from_slice(&((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes());
    }
    let name = path.file_name().map_or("alpha.png".into(), |n| n.to_string_lossy().into_owned());
    let tmp = path.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    let write = || -> Result<(), String> {
        let file = File::create(&tmp).map_err(|e| e.to_string())?;
        let mut enc = png::Encoder::new(io::BufWriter::new(file), n, n);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Sixteen);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&px).map_err(|e| e.to_string())?;
        w.finish().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    };
    write().inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn luminance(p: &[u8]) -> u8 {
    (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32).round() as u8
}

/// macOS: the system tool that registers an app's document types.
const LSREGISTER: &str =
    "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

/// The `.app` bundle the running executable is in (`…/X.app/Contents/MacOS/x`),
/// or None when it isn't run from a bundle.
pub fn app_bundle(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"));
    is_bundle.then(|| bundle.to_path_buf())
}

/// Whether macOS is running the app from a temporary, read-only copy
/// ("App Translocation", for apps opened from a download or a disk image
/// before they're moved): registering that copy would point Finder at a
/// folder that disappears.
pub fn is_translocated(bundle: &std::path::Path) -> bool {
    bundle.components().any(|c| c.as_os_str() == "AppTranslocation")
}

/// macOS: register the running app with Launch Services, in the background,
/// so Finder knows `.oxb` projects belong to it and shows their icon.
///
/// Finder learns an app's document types when it registers the app, which
/// can miss them: an app first opened from the disk image, or translocated
/// on its first launch, leaves projects with the generic "?" document icon.
/// Registering again on every start (a fraction of a second, off the UI
/// thread, with the system's own tool) fixes that. Nothing is sent anywhere.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn register_document_types() {
    let Some(bundle) = std::env::current_exe().ok().and_then(|e| app_bundle(&e)) else { return };
    if is_translocated(&bundle) {
        log::info!(
            "running from a temporary copy ({}); move OxyBrush to Applications so Finder shows its project icons",
            bundle.display()
        );
        return;
    }
    std::thread::spawn(move || match std::process::Command::new(LSREGISTER).arg("-f").arg(&bundle).output() {
        Ok(out) if out.status.success() => log::info!("registered {} for .oxb projects", bundle.display()),
        Ok(out) => log::warn!("lsregister failed ({}): {}", out.status, String::from_utf8_lossy(&out.stderr).trim()),
        Err(e) => log::warn!("could not run lsregister: {e}"),
    });
}

/// The program that shows a folder in the system file manager.
///
/// Folders are opened with it directly, not as a `file://` link: a link goes
/// through the URL handler, which on Windows can be the web browser.
pub fn folder_opener() -> &'static str {
    if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        // In the Flatpak this goes through the desktop's file-manager portal.
        "xdg-open"
    }
}

/// Show `path` in the system file manager (creating it if needed).
pub fn open_folder(path: &std::path::Path) {
    let _ = std::fs::create_dir_all(path);
    if let Err(e) = std::process::Command::new(folder_opener()).arg(path).spawn() {
        log::warn!("could not open {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_query_values() {
        assert_eq!(encode("linux x86_64 (Flatpak)"), "linux%20x86_64%20%28Flatpak%29");
        assert_eq!(encode("0.0.1-alpha"), "0.0.1-alpha");
        assert_eq!(encode("é"), "%C3%A9");
    }

    #[test]
    fn report_url_prefills_fields() {
        let info =
            SystemInfo { gpu: Some("Test GPU".into()), gpu_backend: Some("Vulkan".into()), ..Default::default() };
        let url = info.bug_report_url();
        assert!(url.starts_with("https://github.com/crisprintsstuff/OxyBrush/issues/new?template=bug_report.yml"));
        assert!(url.contains("&gpu=Test%20GPU%20%28Vulkan%29"));
        assert!(info.text().contains("GPU: Test GPU (Vulkan)"));
    }

    #[test]
    fn loads_png_alphas() {
        let dir = std::env::temp_dir().join(format!("oxy-alpha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // RGBA gradient, left black to right white, fully opaque.
        let (w, h) = (32u32, 16u32);
        let mut px = Vec::new();
        for _y in 0..h {
            for x in 0..w {
                let v = (x * 255 / (w - 1)) as u8;
                px.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let path = dir.join("grad.png");
        {
            let f = File::create(&path).unwrap();
            let mut enc = png::Encoder::new(io::BufWriter::new(f), w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&px).unwrap();
        }
        let a = load_alpha_png(&path).unwrap();
        assert_eq!(a.name, "grad");
        assert!(a.sample(glam::Vec2::new(-0.95, 0.0)) < 0.1);
        assert!(a.sample(glam::Vec2::new(0.95, 0.0)) > 0.9);
        std::fs::write(dir.join("bad.png"), b"not a png").unwrap();
        assert!(load_alpha_png(&dir.join("bad.png")).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn an_exported_alpha_reads_back_the_same() {
        let dir = std::env::temp_dir().join(format!("oxy-alpha-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let n = 64u32;
        let data: Vec<f32> =
            (0..n * n).map(|k| ((k % n) as f32 / (n - 1) as f32) * ((k / n) as f32 / (n - 1) as f32)).collect();
        let alpha = oxy_sculpt::Alpha { name: "Capture 1".into(), size: n, data };
        let path = dir.join("capture.png");
        write_alpha_png(&alpha, &path).unwrap();
        // 16-bit grayscale on disk.
        let info =
            png::Decoder::new(io::BufReader::new(File::open(&path).unwrap())).read_info().unwrap().info().clone();
        assert_eq!(
            (info.width, info.height, info.color_type, info.bit_depth),
            (n, n, png::ColorType::Grayscale, png::BitDepth::Sixteen)
        );
        let back = load_alpha_png(&path).unwrap();
        assert_eq!(back.name, "capture");
        for (uv, expect) in [((-0.99, 0.99), 0.0), ((0.99, -0.99), 1.0), ((0.0, 0.0), 0.25)] {
            let (a, b) = (alpha.sample(glam::Vec2::from(uv)), back.sample(glam::Vec2::from(uv)));
            assert!((a - b).abs() < 0.01 && (a - expect).abs() < 0.05, "{uv:?}: {a} vs {b}");
        }
        assert!(std::fs::read_dir(&dir).unwrap().count() == 1, "no temporary file left");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_app_bundle_is_found_from_the_executable() {
        use std::path::Path;
        assert_eq!(
            app_bundle(Path::new("/Applications/OxyBrush.app/Contents/MacOS/oxybrush")).as_deref(),
            Some(Path::new("/Applications/OxyBrush.app"))
        );
        assert_eq!(app_bundle(Path::new("/usr/local/bin/oxybrush")), None, "not in a bundle");
        assert_eq!(app_bundle(Path::new("/tmp/OxyBrush/Contents/MacOS/oxybrush")), None, "no .app");
        assert!(is_translocated(Path::new("/private/var/folders/x/T/AppTranslocation/ABC-123/d/OxyBrush.app")));
        assert!(!is_translocated(Path::new("/Applications/OxyBrush.app")));
    }

    #[test]
    fn folders_open_in_the_file_manager_not_the_browser() {
        let opener = folder_opener();
        #[cfg(target_os = "windows")]
        assert_eq!(opener, "explorer");
        #[cfg(target_os = "macos")]
        assert_eq!(opener, "open");
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        assert_eq!(opener, "xdg-open");
    }
}
