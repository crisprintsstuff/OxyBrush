//! `.oxyaddon` packages: one file that installs a plugin.
//!
//! An `.oxyaddon` is a ZIP archive holding a plugin folder's files, with
//! `plugin.json` at the top. Its `"kind"` says what it is:
//!
//! - `"wasm"` (the default): an in-app WebAssembly plugin (`plugin.wasm` or
//!   `plugin.wat`), run sandboxed inside OxyBrush;
//! - `"python"`: a Python script add-on; `"entry"` names the script (default
//!   `main.py`). It runs as a separate process through the local agent
//!   server, like any Python plugin.
//!
//! Installing extracts the archive into `<plugins>/<id>/`, where `id` is
//! the manifest's `"id"` (or its name), made filesystem-safe.
//!
//! INVARIANT: nothing is written outside `<plugins>/<id>/`. Entry paths
//! must be relative with no `..`, the archive is size- and count-limited
//! (no zip bombs), and the folder is filled under a temporary name and then
//! renamed into place, so a failed install leaves no half-written plugin.
//!
//! Only the parts of ZIP that plugins need are read: stored (method 0) and
//! deflated (method 8) entries, no encryption, no ZIP64. CRCs are checked.

use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

/// File extension of add-on packages.
pub const EXTENSION: &str = "oxyaddon";
/// Most files in one add-on.
pub const MAX_FILES: usize = 512;
/// Most bytes one add-on may unpack to.
pub const MAX_UNPACKED: usize = 256 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddonKind {
    Wasm,
    Python,
}

/// What an add-on is, from its `plugin.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct AddonInfo {
    /// Folder name it installs to.
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub kind: AddonKind,
    /// Python: the script to run, relative to the add-on folder.
    pub entry: Option<String>,
}

/// The kind and Python entry script named by a `plugin.json` (also used
/// for installed folders). RETURNS: (kind, entry).
pub fn manifest_kind(manifest: &str) -> Result<(AddonKind, Option<String>), String> {
    let v: serde_json::Value = serde_json::from_str(manifest).map_err(|e| format!("plugin.json: {e}"))?;
    match v.get("kind").and_then(|k| k.as_str()).unwrap_or("wasm") {
        "wasm" => Ok((AddonKind::Wasm, None)),
        "python" => {
            let entry = v.get("entry").and_then(|e| e.as_str()).unwrap_or("main.py").to_owned();
            safe_relative(&entry)?;
            if !entry.to_ascii_lowercase().ends_with(".py") {
                return Err(format!("plugin.json: \"entry\" must be a .py file, not {entry:?}"));
            }
            Ok((AddonKind::Python, Some(entry)))
        }
        other => Err(format!("plugin.json: unknown \"kind\" {other:?} (use \"wasm\" or \"python\")")),
    }
}

/// A file name made safe as a folder name.
fn safe_id(s: &str) -> String {
    let id: String = s
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '_' })
        .collect();
    let id = id.trim_matches('_').to_owned();
    if id.is_empty() { "addon".into() } else { id.chars().take(64).collect() }
}

/// `p` as a relative path inside its root (no absolute paths, no `..`).
fn safe_relative(p: &str) -> Result<PathBuf, String> {
    let path = Path::new(p);
    if p.is_empty()
        || p.contains('\\')
        || path.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!("unsafe path in add-on: {p:?}"));
    }
    Ok(path.to_path_buf())
}

// ---- ZIP ----------------------------------------------------------------
//
// Local file header (little-endian), signature 0x04034b50:
//   0 sig u32 | 4 version u16 | 6 flags u16 | 8 method u16 | 10 time u16
//   12 date u16 | 14 crc u32 | 18 csize u32 | 22 usize u32 | 26 nlen u16
//   28 xlen u16 | 30 name | extra | data
// Central directory header, signature 0x02014b50:
//   0 sig | 4 made-by u16 | 6 version u16 | 8 flags u16 | 10 method u16
//   12 time | 14 date | 16 crc u32 | 20 csize u32 | 24 usize u32
//   28 nlen u16 | 30 xlen u16 | 32 clen u16 | 34 disk u16 | 36 iattr u16
//   38 eattr u32 | 42 local offset u32 | 46 name
// End of central directory, signature 0x06054b50:
//   0 sig | 4 disk u16 | 6 cd disk u16 | 8 entries here u16 | 10 entries u16
//   12 cd size u32 | 16 cd offset u32 | 20 comment len u16

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Files of an archive: (path inside it, contents).
pub type Files = Vec<(String, Vec<u8>)>;

/// The files of a ZIP archive: (path, contents), directories left out.
pub fn read_archive(bytes: &[u8]) -> Result<Files, String> {
    let bad = |m: &str| format!("not a valid add-on archive: {m}");
    // The end-of-central-directory record is within the last 64 KiB + 22 bytes.
    let from = bytes.len().saturating_sub(22 + 65_535);
    let eocd = (from..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| u32_at(bytes, i) == Some(0x0605_4b50))
        .ok_or_else(|| bad("no end record"))?;
    let count = u16_at(bytes, eocd + 10).ok_or_else(|| bad("short end record"))? as usize;
    let mut at = u32_at(bytes, eocd + 16).ok_or_else(|| bad("short end record"))? as usize;
    if count > MAX_FILES {
        return Err(format!("the add-on has more than {MAX_FILES} files"));
    }
    let mut files = Vec::with_capacity(count);
    let mut total = 0usize;
    for _ in 0..count {
        if u32_at(bytes, at) != Some(0x0201_4b50) {
            return Err(bad("broken central directory"));
        }
        let field = |o: usize| u16_at(bytes, at + o).ok_or_else(|| bad("short header"));
        let flags = field(8)?;
        let method = field(10)?;
        let crc = u32_at(bytes, at + 16).ok_or_else(|| bad("short header"))?;
        let csize = u32_at(bytes, at + 20).ok_or_else(|| bad("short header"))? as usize;
        let usize_ = u32_at(bytes, at + 24).ok_or_else(|| bad("short header"))? as usize;
        let (nlen, xlen, clen) = (field(28)? as usize, field(30)? as usize, field(32)? as usize);
        let local = u32_at(bytes, at + 42).ok_or_else(|| bad("short header"))? as usize;
        let name = bytes.get(at + 46..at + 46 + nlen).ok_or_else(|| bad("short name"))?;
        let name = String::from_utf8(name.to_vec()).map_err(|_| bad("file name is not UTF-8"))?;
        at += 46 + nlen + xlen + clen;
        if name.ends_with('/') {
            continue;
        }
        if flags & 1 != 0 {
            return Err(format!("{name} is encrypted; add-ons can't be"));
        }
        total = total.saturating_add(usize_);
        if total > MAX_UNPACKED {
            return Err(format!("the add-on unpacks to more than {} MB", MAX_UNPACKED >> 20));
        }
        if u32_at(bytes, local) != Some(0x0403_4b50) {
            return Err(bad("broken local header"));
        }
        let lname = u16_at(bytes, local + 26).ok_or_else(|| bad("short header"))? as usize;
        let lextra = u16_at(bytes, local + 28).ok_or_else(|| bad("short header"))? as usize;
        let start = local + 30 + lname + lextra;
        let data = bytes.get(start..start + csize).ok_or_else(|| bad("truncated data"))?;
        let content = match method {
            0 => data.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(data, usize_)
                .map_err(|_| format!("{name}: could not be decompressed"))?,
            m => return Err(format!("{name}: compression method {m} is not supported (use store or deflate)")),
        };
        if content.len() != usize_ || crc32fast::hash(&content) != crc {
            return Err(format!("{name}: damaged (size or checksum mismatch)"));
        }
        safe_relative(&name)?;
        files.push((name, content));
    }
    Ok(files)
}

/// Build a ZIP archive (deflated entries) from (path, contents).
pub fn write_archive(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, content) in files {
        let packed = miniz_oxide::deflate::compress_to_vec(content, 8);
        let crc = crc32fast::hash(content);
        let offset = out.len() as u32;
        let n = name.as_bytes();
        let header = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend(sig.to_le_bytes());
            if central {
                h.extend(20u16.to_le_bytes()); // made by
            }
            h.extend(20u16.to_le_bytes()); // version needed
            h.extend(0x0800u16.to_le_bytes()); // flags: UTF-8 names
            h.extend(8u16.to_le_bytes()); // deflate
            h.extend(0u16.to_le_bytes()); // time
            h.extend(0x21u16.to_le_bytes()); // date: 1980-01-01
            h.extend(crc.to_le_bytes());
            h.extend((packed.len() as u32).to_le_bytes());
            h.extend((content.len() as u32).to_le_bytes());
            h.extend((n.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes()); // extra length
            if central {
                h.extend(0u16.to_le_bytes()); // comment length
                h.extend(0u16.to_le_bytes()); // disk
                h.extend(0u16.to_le_bytes()); // internal attributes
                h.extend(0u32.to_le_bytes()); // external attributes
                h.extend(offset.to_le_bytes());
            }
            h.extend(n);
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend(&packed);
        central.extend(header(0x0201_4b50, true));
    }
    let cd_offset = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(cd_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

// ---- Add-ons ------------------------------------------------------------

/// Read and check an add-on: its files and what it is.
pub fn inspect(bytes: &[u8]) -> Result<(AddonInfo, Files), String> {
    let files = read_archive(bytes)?;
    let manifest =
        files.iter().find(|(n, _)| n == "plugin.json").ok_or("the add-on has no plugin.json at its top level")?;
    let text = std::str::from_utf8(&manifest.1).map_err(|_| "plugin.json is not UTF-8")?;
    let (kind, entry) = manifest_kind(text)?;
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("plugin.json: {e}"))?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
    let name = s("name");
    if name.trim().is_empty() {
        return Err("plugin.json: missing \"name\"".into());
    }
    let has = |f: &str| files.iter().any(|(n, _)| n == f);
    match kind {
        AddonKind::Wasm => {
            // Check it the way it will be loaded.
            let module = files
                .iter()
                .find(|(n, _)| n == "plugin.wasm" || n == "plugin.wat")
                .ok_or("a WebAssembly add-on needs plugin.wasm or plugin.wat")?;
            crate::Plugin::from_parts(text, &module.1, Path::new("."))?;
        }
        AddonKind::Python => {
            let entry = entry.as_deref().unwrap_or("main.py");
            if !has(entry) {
                return Err(format!("the add-on's entry script {entry} is missing"));
            }
        }
    }
    let id = safe_id(&if s("id").is_empty() { name.clone() } else { s("id") });
    Ok((AddonInfo { id, name, version: s("version"), description: s("description"), kind, entry }, files))
}

/// Install an add-on into `plugins_dir/<id>/`, replacing an older install
/// of the same id. RETURNS: what was installed and where.
pub fn install(bytes: &[u8], plugins_dir: &Path) -> Result<(AddonInfo, PathBuf), String> {
    let (info, files) = inspect(bytes)?;
    let target = plugins_dir.join(&info.id);
    let staging = plugins_dir.join(format!(".installing-{}", info.id));
    let old = plugins_dir.join(format!(".replaced-{}", info.id));
    let io_err = |e: io::Error| format!("could not install {}: {e}", info.name);
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_dir_all(&old);
    std::fs::create_dir_all(&staging).map_err(io_err)?;
    let written = (|| -> io::Result<()> {
        for (name, content) in &files {
            let path = staging.join(safe_relative(name).map_err(io::Error::other)?);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::File::create(&path)?.write_all(content)?;
        }
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(io_err(e));
    }
    if target.exists() {
        std::fs::rename(&target, &old).map_err(io_err)?;
    }
    if let Err(e) = std::fs::rename(&staging, &target) {
        // Put the previous version back.
        let _ = std::fs::rename(&old, &target);
        let _ = std::fs::remove_dir_all(&staging);
        return Err(io_err(e));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok((info, target))
}

/// Pack a plugin folder (with `plugin.json` at its top) into `.oxyaddon`
/// bytes, checking it as `install` would. Hidden files are left out.
pub fn pack(folder: &Path) -> Result<Vec<u8>, String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> io::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if name.starts_with('.') || name == "__pycache__" {
                continue;
            }
            if p.is_dir() {
                walk(root, &p, out)?;
            } else {
                let rel = p.strip_prefix(root).unwrap_or(&p);
                let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                out.push((rel.join("/"), std::fs::read(&p)?));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(folder, folder, &mut files).map_err(|e| format!("{}: {e}", folder.display()))?;
    let bytes = write_archive(&files);
    inspect(&bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUASH_JSON: &str = include_str!("../plugins/squash/plugin.json");
    const SQUASH_WAT: &str = include_str!("../plugins/squash/plugin.wat");

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oxy-addon-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn archives_round_trip_and_damage_is_caught() {
        let files = vec![
            ("plugin.json".to_string(), b"{}".to_vec()),
            ("lib/data.bin".to_string(), (0..5000u32).flat_map(|i| (i % 7).to_le_bytes()).collect()),
        ];
        let bytes = write_archive(&files);
        assert_eq!(read_archive(&bytes).unwrap(), files);
        let mut broken = bytes.clone();
        broken[30 + "plugin.json".len() + 1] ^= 0xff; // inside the first entry's compressed data
        assert!(read_archive(&broken).is_err());
        assert!(read_archive(b"not a zip").is_err());
        for unsafe_name in ["../evil.py", "/etc/x", "a/../../b", "c:\\win"] {
            let z = write_archive(&[(unsafe_name.to_string(), b"x".to_vec())]);
            assert!(read_archive(&z).is_err(), "accepted {unsafe_name}");
        }
    }

    #[test]
    fn wasm_and_python_addons_install_and_replace() {
        let src = tmp("src");
        std::fs::write(src.join("plugin.json"), SQUASH_JSON).unwrap();
        std::fs::write(src.join("plugin.wat"), SQUASH_WAT).unwrap();
        std::fs::write(src.join(".hidden"), "skip").unwrap();
        let bytes = pack(&src).unwrap();
        let plugins = tmp("plugins");
        let (info, dir) = install(&bytes, &plugins).unwrap();
        assert_eq!((info.id.as_str(), info.kind), ("squash", AddonKind::Wasm));
        assert!(dir.join("plugin.wat").is_file() && !dir.join(".hidden").exists());
        assert_eq!(crate::discover(&plugins).len(), 1, "loads as an in-app plugin");

        // A Python add-on, with its own id, installed twice (the second replaces the first).
        let py = tmp("py");
        std::fs::write(
            py.join("plugin.json"),
            r#"{"name":"Mesh Report","id":"mesh-report","kind":"python","entry":"report.py","description":"Prints stats"}"#,
        )
        .unwrap();
        std::fs::write(py.join("report.py"), "print('v1')\n").unwrap();
        let (info, dir) = install(&pack(&py).unwrap(), &plugins).unwrap();
        assert_eq!(
            (info.id.as_str(), info.kind, info.entry.as_deref()),
            ("mesh-report", AddonKind::Python, Some("report.py"))
        );
        std::fs::write(py.join("report.py"), "print('v2')\n").unwrap();
        install(&pack(&py).unwrap(), &plugins).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("report.py")).unwrap(), "print('v2')\n");
        assert_eq!(crate::discover(&plugins).len(), 1, "Python add-ons are not loaded as WebAssembly");

        // Broken add-ons are refused before anything is written.
        std::fs::write(py.join("plugin.json"), r#"{"name":"X","kind":"python","entry":"missing.py"}"#).unwrap();
        assert!(pack(&py).unwrap_err().contains("missing.py"));
        assert!(
            inspect(&write_archive(&[("plugin.json".into(), br#"{"name":"X","kind":"lua"}"#.to_vec())]))
                .unwrap_err()
                .contains("lua")
        );
        assert!(inspect(&write_archive(&[("readme.txt".into(), b"hi".to_vec())])).unwrap_err().contains("plugin.json"));
        for d in [src, plugins, py] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
