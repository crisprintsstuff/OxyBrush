//! The user's brush preset library: one `.oxbrush` file per preset in the
//! per-user data folder (`…/oxybrush/brushes`, or `%LOCALAPPDATA%\OxyBrush\
//! brushes` on Windows). Writes are atomic (temp file + rename).

use oxy_sculpt::BrushPreset;
use oxy_sculpt::preset::{EXTENSION, FILE_HEADER};
use std::path::{Path, PathBuf};

pub fn library_dir() -> Option<PathBuf> {
    let app = if cfg!(windows) { "OxyBrush" } else { "oxybrush" };
    Some(dirs::data_local_dir()?.join(app).join("brushes"))
}

/// A file name for a preset name: letters, digits, spaces, `-` and `_` kept.
pub fn file_name(name: &str) -> String {
    let clean: String =
        name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    let clean = clean.trim();
    let clean = if clean.is_empty() { "brush" } else { clean };
    format!("{}.{EXTENSION}", clean.chars().take(80).collect::<String>())
}

/// All presets in `dir`, sorted by name. Unreadable files are skipped.
pub fn load_all(dir: &Path) -> Vec<BrushPreset> {
    let mut out: Vec<BrushPreset> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == EXTENSION))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|t| BrushPreset::from_text(&t).ok())
        .collect();
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

/// Write `preset` to `path` atomically.
pub fn write_file(preset: &BrushPreset, path: &Path) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension(format!("{EXTENSION}.tmp"));
    std::fs::write(&tmp, preset.to_text())?;
    std::fs::rename(&tmp, path)
}

/// Save into the library, replacing a preset of the same name.
pub fn save(dir: &Path, preset: &BrushPreset) -> std::io::Result<PathBuf> {
    let path = dir.join(file_name(&preset.name));
    write_file(preset, &path)?;
    Ok(path)
}

/// Remove a preset from the library (by name).
pub fn delete(dir: &Path, name: &str) -> std::io::Result<()> {
    std::fs::remove_file(dir.join(file_name(name)))
}

/// Read a preset file chosen by the user (`.oxbrush`).
pub fn read_file(path: &Path) -> Result<BrushPreset, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if !text.trim_start().starts_with(FILE_HEADER) && !text.contains("brush =") {
        return Err("not an OxyBrush brush preset".into());
    }
    BrushPreset::from_text(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_sculpt::BrushKind;

    #[test]
    fn library_save_load_replace_delete() {
        let dir = std::env::temp_dir().join(format!("oxy-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_all(&dir).is_empty(), "missing folder = empty library");
        let mut a = BrushPreset::new("Zeta clay", BrushKind::Clay);
        let b = BrushPreset::new("alpha/../../escape", BrushKind::Draw);
        save(&dir, &a).unwrap();
        let path_b = save(&dir, &b).unwrap();
        assert_eq!(path_b.parent().unwrap(), dir, "names can't leave the library folder");
        a.strength = 0.9;
        save(&dir, &a).unwrap(); // replaces
        std::fs::write(dir.join("junk.oxbrush"), "not a preset").unwrap();
        let all = load_all(&dir);
        assert_eq!(all.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["alpha/../../escape", "Zeta clay"]);
        assert_eq!(all[1].strength, 0.9);
        assert_eq!(read_file(&path_b).unwrap(), b);
        delete(&dir, "Zeta clay").unwrap();
        assert_eq!(load_all(&dir).len(), 1);
        assert!(!dir.join("Zeta clay.oxbrush.tmp").exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
