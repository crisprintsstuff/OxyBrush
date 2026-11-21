//! Recipe Box library manager (RFC 0026: Recipe Box).
//!
//! Stores `.oxydetail` files and their thumbnail PNGs in the user's data folder
//! (`…/oxybrush/parts` or `%LOCALAPPDATA%\OxyBrush\parts`).
//! `library.txt` tracks order, categories, and content hashes.

use glam::Vec3;
use oxy_sculpt::PartDetail;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "oxydetail";
const LIBRARY_FILE: &str = "library.txt";

/// An active part placement being dragged or positioned on the surface.
#[derive(Clone, Debug, PartialEq)]
pub struct PartPlacement {
    pub detail: PartDetail,
    pub anchor: Vec3,
    pub normal: Vec3,
    pub tangent: Vec3,
    pub twist: f32,
    pub scale: f32,
    pub piece: u64,
    pub choice_open: bool,
    pub on_plane: bool,
    pub mirror_axis: u8, // 1 = X, 2 = Y, 3 = Z
}

/// Placement choice on release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceChoice {
    One,
    MirrorPair,
    OneCentered,
    MirrorHalvesMatch,
}

/// Where the parts library lives on disk.
pub fn library_dir() -> Option<PathBuf> {
    let app = if cfg!(windows) { "OxyBrush" } else { "oxybrush" };
    Some(dirs::data_local_dir()?.join(app).join("parts"))
}

/// A sanitized file name for a part: `<clean_name>.oxydetail`.
pub fn file_name(name: &str) -> String {
    let clean: String =
        name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    let clean = clean.trim();
    let clean = if clean.is_empty() { "part" } else { clean };
    format!("{}.{EXTENSION}", clean.chars().take(80).collect::<String>())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// Compute SHA-256 of byte slice.
pub fn content_hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// An entry in the user's Recipe Box.
#[derive(Clone, Debug, PartialEq)]
pub struct PartEntry {
    pub name: String,
    pub category: String,
    pub file_name: String,
    pub hash: [u8; 32],
    pub detail: PartDetail,
    pub thumbnail_path: Option<PathBuf>,
}

/// The loaded parts library.
#[derive(Clone, Debug, Default)]
pub struct PartsLibrary {
    pub entries: Vec<PartEntry>,
    pub selected_category: String, // "All" or category name
}

impl PartsLibrary {
    /// Distinct non-empty categories present in the library, sorted.
    pub fn categories(&self) -> Vec<String> {
        let mut cats: Vec<String> =
            self.entries.iter().map(|e| e.category.trim().to_string()).filter(|c| !c.is_empty()).collect();
        cats.sort();
        cats.dedup();
        cats
    }

    /// Entries filtered by the active category filter ("All" shows everything).
    pub fn filtered_entries(&self) -> Vec<&PartEntry> {
        if self.selected_category.is_empty() || self.selected_category == "All" {
            self.entries.iter().collect()
        } else {
            self.entries.iter().filter(|e| e.category == self.selected_category).collect()
        }
    }

    /// Load the entire library from `dir`.
    pub fn load_all(dir: &Path) -> Self {
        let _ = std::fs::create_dir_all(dir);
        let lib_path = dir.join(LIBRARY_FILE);
        let lib_text = std::fs::read_to_string(&lib_path).unwrap_or_default();

        let mut order: Vec<String> = Vec::new();
        let mut categories: HashMap<String, String> = HashMap::new();
        let mut hashes: HashMap<String, [u8; 32]> = HashMap::new();

        for line in lib_text.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let k = k.trim();
                let v = v.trim();
                if k == "order" {
                    order = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                } else if let Some(fname) = k.strip_prefix("category.") {
                    categories.insert(fname.trim().to_string(), v.to_string());
                } else if let Some(fname) = k.strip_prefix("hash.")
                    && let Some(bytes) = hex_decode(v)
                    && bytes.len() == 32
                {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    hashes.insert(fname.trim().to_string(), arr);
                }
            }
        }

        // Read all .oxydetail files in dir
        let mut disk_files: HashMap<String, (PartDetail, [u8; 32])> = HashMap::new();
        if let Ok(read_dir) = std::fs::read_dir(dir) {
            for entry in read_dir.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|ext| ext == EXTENSION)
                    && let Some(fname) = p.file_name().and_then(|f| f.to_str())
                    && let Ok(bytes) = std::fs::read(&p)
                    && let Ok(detail) = PartDetail::from_bytes(&bytes)
                {
                    let hash = content_hash(&bytes);
                    disk_files.insert(fname.to_string(), (detail, hash));
                }
            }
        }

        let mut entries = Vec::new();

        // 1. Add ordered files first
        for fname in &order {
            if let Some((detail, hash)) = disk_files.remove(fname) {
                let category = categories.get(fname).cloned().unwrap_or_default();
                let thumb = dir.join(format!("{}.png", fname.strip_suffix(&format!(".{EXTENSION}")).unwrap_or(fname)));
                let thumb_path = if thumb.exists() { Some(thumb) } else { None };
                entries.push(PartEntry {
                    name: detail.name.clone(),
                    category,
                    file_name: fname.clone(),
                    hash,
                    detail,
                    thumbnail_path: thumb_path,
                });
            }
        }

        // 2. Append any un-ordered files found in directory
        let mut remaining_keys: Vec<String> = disk_files.keys().cloned().collect();
        remaining_keys.sort();
        for fname in remaining_keys {
            let (detail, hash) = disk_files.remove(&fname).unwrap();
            let category = categories.get(&fname).cloned().unwrap_or_default();
            let thumb = dir.join(format!("{}.png", fname.strip_suffix(&format!(".{EXTENSION}")).unwrap_or(&fname)));
            let thumb_path = if thumb.exists() { Some(thumb) } else { None };
            entries.push(PartEntry {
                name: detail.name.clone(),
                category,
                file_name: fname,
                hash,
                detail,
                thumbnail_path: thumb_path,
            });
        }

        PartsLibrary { entries, selected_category: "All".into() }
    }

    /// Save `library.txt` atomically.
    pub fn save_index(&self, dir: &Path) -> std::io::Result<()> {
        let _ = std::fs::create_dir_all(dir);
        let lib_path = dir.join(LIBRARY_FILE);
        let tmp_path = dir.join("library.txt.tmp");

        let mut out = String::from("# OxyBrush parts library.\nversion = 1\n");
        let order_str = self.entries.iter().map(|e| e.file_name.as_str()).collect::<Vec<_>>().join(", ");
        out.push_str(&format!("order = {order_str}\n"));

        for e in &self.entries {
            if !e.category.is_empty() {
                out.push_str(&format!("category.{} = {}\n", e.file_name, e.category));
            }
            out.push_str(&format!("hash.{} = {}\n", e.file_name, hex_encode(&e.hash)));
        }

        std::fs::write(&tmp_path, out)?;
        std::fs::rename(&tmp_path, lib_path)
    }

    /// Add or update a part in the library.
    pub fn save_part(&mut self, dir: &Path, detail: PartDetail, category: &str) -> std::io::Result<PathBuf> {
        let _ = std::fs::create_dir_all(dir);
        let fname = file_name(&detail.name);
        let file_path = dir.join(&fname);
        let tmp_path = file_path.with_extension(format!("{EXTENSION}.tmp"));

        let bytes = detail.to_bytes();
        let hash = content_hash(&bytes);

        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &file_path)?;

        // Update entry list
        self.entries.retain(|e| e.file_name != fname);
        self.entries.push(PartEntry {
            name: detail.name.clone(),
            category: category.trim().to_string(),
            file_name: fname,
            hash,
            detail,
            thumbnail_path: None,
        });

        let _ = self.save_index(dir);
        Ok(file_path)
    }

    /// Delete a part from the library by file name.
    #[allow(dead_code)]
    pub fn delete_part(&mut self, dir: &Path, file_name: &str) -> std::io::Result<()> {
        let file_path = dir.join(file_name);
        let _ = std::fs::remove_file(file_path);
        let thumb_path =
            dir.join(format!("{}.png", file_name.strip_suffix(&format!(".{EXTENSION}")).unwrap_or(file_name)));
        let _ = std::fs::remove_file(thumb_path);

        self.entries.retain(|e| e.file_name != file_name);
        self.save_index(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use oxy_sculpt::BrushSettings;
    use oxy_sculpt::Symmetry;
    use oxy_sculpt::recipe::{RecipeEvent, StrokeRecipe};

    #[test]
    fn library_save_and_reload() {
        let dir = std::env::temp_dir().join(format!("oxy_parts_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut lib = PartsLibrary::default();
        let recipe = StrokeRecipe {
            settings: BrushSettings::default(),
            invert: false,
            symmetry: Symmetry::default(),
            center: Vec3::ZERO,
            view: Vec3::Z,
            grab_through: false,
            paint: Vec3::ONE,
            alpha: None,
            events: vec![RecipeEvent::Sample {
                sample: oxy_sculpt::StrokeSample { position: Vec3::ZERO, normal: Vec3::Y, pressure: 1.0 },
                spacing: 0.1,
                strength: 1.0,
                max_dabs: 0,
            }],
        };
        let detail =
            PartDetail { name: "Horn".into(), reference_radius: 0.3, recipes: vec![recipe], alphas: Vec::new() };

        lib.save_part(&dir, detail, "Creatures").expect("saves part");
        assert_eq!(lib.entries.len(), 1);

        // Reload
        let reloaded = PartsLibrary::load_all(&dir);
        assert_eq!(reloaded.entries.len(), 1);
        assert_eq!(reloaded.entries[0].name, "Horn");
        assert_eq!(reloaded.entries[0].category, "Creatures");
        assert_eq!(reloaded.categories(), vec!["Creatures"]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
