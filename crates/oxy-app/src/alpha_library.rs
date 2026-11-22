//! Imported brush alphas shared by all tabs and persisted between sessions.

use fs4::fs_std::FileExt;
use oxy_sculpt::{Alpha, BuiltinAlpha};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Imported {
    pub alpha: Alpha,
    pub library_error: Option<String>,
}

pub fn import(path: &Path, dir: Option<&Path>) -> Result<Imported, String> {
    let mut alpha =
        crate::support::load_alpha_png(path).map_err(|err| format!("Could not load {}: {err}", path.display()))?;
    let library_error = match dir {
        Some(dir) => store(dir, &mut alpha).err(),
        None => Some("No user alpha folder is available".into()),
    };
    Ok(Imported { alpha, library_error })
}

pub fn library_dir() -> Option<PathBuf> {
    let app = if cfg!(windows) { "OxyBrush" } else { "oxybrush" };
    Some(dirs::data_local_dir()?.join(app).join("alphas"))
}

fn pngs(dir: &Path) -> impl Iterator<Item = PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("png")))
}

pub fn load_all(dir: &Path) -> Vec<Alpha> {
    let mut paths: Vec<_> = pngs(dir).collect();
    paths.sort_by_cached_key(|path| path.file_stem().unwrap_or_default().to_string_lossy().to_lowercase());
    paths
        .iter()
        .filter_map(|path| {
            crate::support::load_alpha_png(path)
                .map_err(|err| log::warn!("Skipping alpha {}: {err}", path.display()))
                .ok()
        })
        .collect()
}

/// Save the imported raster atomically. Multiple OxyBrush processes can
/// import at the same time, so hold the directory's lock while choosing
/// its filename and publishing the PNG.
fn store(dir: &Path, alpha: &mut Alpha) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let lock = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(".import.lock"))
        .map_err(|err| err.to_string())?;
    FileExt::lock_exclusive(&lock).map_err(|err| err.to_string())?;

    let mut taken: HashSet<_> = BuiltinAlpha::ALL.iter().map(|alpha| alpha.label().to_lowercase()).collect();
    taken.insert("none".into());
    taken.extend(pngs(dir).filter_map(|path| path.file_stem().map(|name| name.to_string_lossy().to_lowercase())));
    let clean: String = alpha
        .name
        .chars()
        .take(56)
        .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') { c } else { '_' })
        .collect();
    let base = clean.trim();
    let base = if base.is_empty() { "Alpha" } else { base };
    let valid = |name: &str| !taken.contains(&name.to_lowercase()) && !dir.join(format!("{name}.png")).exists();
    // A Windows device name can never be used as a bare file stem. Prefix
    // these names, including the Unicode superscript digits Windows accepts.
    let upper = base.to_ascii_uppercase();
    let device_number = upper.strip_prefix("COM").or_else(|| upper.strip_prefix("LPT"));
    let device = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || device_number.is_some_and(|suffix| {
            matches!(suffix, "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
        });
    let mut name = if device { format!("Alpha {base}") } else { base.to_owned() };
    let first_name = name.clone();
    for suffix in 2.. {
        if valid(&name) {
            break;
        }
        name = format!("{first_name} {suffix}");
    }

    crate::support::write_alpha_png(alpha, &dir.join(format!("{name}.png")))?;
    alpha.name = name;
    Ok(())
}
