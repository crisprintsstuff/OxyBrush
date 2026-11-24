//! Recipe Box library manager (RFC 0026: Recipe Box).
//!
//! Stores `.oxydetail` files and their thumbnail PNGs in the user's data folder
//! (`…/oxybrush/parts` or `%LOCALAPPDATA%\OxyBrush\parts`).
//! `library.txt` tracks order, display names, categories, and content hashes.

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
    /// SHA-256 of the library file the recipe came from, so Save back to
    /// Recipe Box can find it again (zero when unknown).
    pub content: [u8; 32],
    /// Moving a placed recipe (Move on surface) rather than dropping a new
    /// one: its id and frame when the move started.
    pub moving: Option<(u64, oxy_sculpt::PartFrame)>,
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
    pub selected_category: String, // Empty shows all recipes
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
        if self.selected_category.is_empty() {
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
        let mut names = HashMap::new();

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
                } else if let Some(fname) = k.strip_prefix("name.") {
                    names.insert(fname.trim().to_string(), v.to_string());
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
                    name: names.remove(fname).unwrap_or_else(|| detail.name.clone()),
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
                name: names.remove(&fname).unwrap_or_else(|| detail.name.clone()),
                category,
                file_name: fname,
                hash,
                detail,
                thumbnail_path: thumb_path,
            });
        }

        PartsLibrary { entries, selected_category: String::new() }
    }

    /// Save `library.txt` atomically.
    pub fn save_index(&self, dir: &Path) -> std::io::Result<()> {
        let _ = std::fs::create_dir_all(dir);
        let lib_path = dir.join(LIBRARY_FILE);

        let mut out = String::from("# OxyBrush parts library.\nversion = 1\n");
        let order_str = self.entries.iter().map(|e| e.file_name.as_str()).collect::<Vec<_>>().join(", ");
        out.push_str(&format!("order = {order_str}\n"));

        for e in &self.entries {
            if e.name != e.detail.name {
                out.push_str(&format!("name.{} = {}\n", e.file_name, e.name));
            }
            if !e.category.is_empty() {
                out.push_str(&format!("category.{} = {}\n", e.file_name, e.category));
            }
            out.push_str(&format!("hash.{} = {}\n", e.file_name, hex_encode(&e.hash)));
        }

        oxy_io::write_atomic(&lib_path, out.as_bytes())
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
            category: category.trim().replace(['\r', '\n'], " "),
            file_name: fname,
            hash,
            detail,
            thumbnail_path: None,
        });

        let _ = self.save_index(dir);
        Ok(file_path)
    }

    /// A name no entry uses yet: `name`, or `name 2`, `name 3`… (Save as
    /// new recipe must not replace another recipe with the same name).
    pub fn free_name(&self, name: &str) -> String {
        let taken = |n: &str| {
            let file = file_name(n);
            self.entries.iter().any(|e| e.file_name.eq_ignore_ascii_case(&file) || e.name.eq_ignore_ascii_case(n))
        };
        if !taken(name) {
            return name.to_string();
        }
        (2..)
            .map(|k| {
                let suffix = format!(" {k}");
                let prefix: String = name.chars().take(80 - suffix.len()).collect();
                format!("{prefix}{suffix}")
            })
            .find(|n| !taken(n))
            .expect("a free name")
    }

    /// Replace the detail in entry `file_name` with `detail`, keeping the
    /// entry's name, category and file (RFC 0026: Save back to Recipe Box).
    /// The write is atomic (temporary file, then rename).
    ///
    /// RETURNS: the file's new content hash.
    pub fn replace_part(&mut self, dir: &Path, file_name: &str, mut detail: PartDetail) -> std::io::Result<[u8; 32]> {
        let Some(at) = self.entries.iter().position(|e| e.file_name == file_name) else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no such recipe in the Recipe Box"));
        };
        // Display names in the index do not change the backing recipe identity.
        detail.name = self.entries[at].detail.name.clone();
        let file_path = dir.join(file_name);
        let tmp_path = file_path.with_extension(format!("{EXTENSION}.tmp"));
        let bytes = detail.to_bytes();
        let hash = content_hash(&bytes);
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &file_path)?;
        let e = &mut self.entries[at];
        e.hash = hash;
        e.detail = detail;
        let _ = self.save_index(dir);
        Ok(hash)
    }

    /// Change the library label and category without replacing the detail file.
    pub fn edit_part(&mut self, dir: &Path, file_name: &str, name: &str, category: &str) -> std::io::Result<()> {
        let at = self.entry_index(file_name)?;
        let (name, category) = (name.trim(), category.trim());
        if name.is_empty() || [name, category].iter().any(|s| s.contains(['\r', '\n'])) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Enter a name and a single-line category",
            ));
        }
        if self.entries.iter().enumerate().any(|(i, e)| i != at && e.name.eq_ignore_ascii_case(name)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "That recipe name is already in the box",
            ));
        }
        let old_name = std::mem::replace(&mut self.entries[at].name, name.into());
        let old_category = std::mem::replace(&mut self.entries[at].category, category.into());
        if let Err(error) = self.save_index(dir) {
            self.entries[at].name = old_name;
            self.entries[at].category = old_category;
            return Err(error);
        }
        self.reset_empty_filter();
        Ok(())
    }

    /// Move a recipe to a library position and persist the new order.
    pub fn reorder_part(&mut self, dir: &Path, from: usize, to: usize) -> std::io::Result<()> {
        if from == to || from >= self.entries.len() || to >= self.entries.len() {
            return Ok(());
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
        if let Err(error) = self.save_index(dir) {
            let entry = self.entries.remove(to);
            self.entries.insert(from, entry);
            return Err(error);
        }
        Ok(())
    }

    /// Delete the library file. Placed recipes are stored in their projects.
    pub fn delete_part(&mut self, dir: &Path, file_name: &str) -> std::io::Result<()> {
        let at = self.entry_index(file_name)?;
        let file = dir.join(&self.entries[at].file_name);
        let pending = file.with_extension(format!("deleting-{}", std::process::id()));
        let moved = match std::fs::rename(&file, &pending) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        let entry = self.entries.remove(at);
        if let Err(error) = self.save_index(dir) {
            self.entries.insert(at, entry);
            if moved {
                std::fs::rename(pending, file)?;
            }
            return Err(error);
        }
        if moved {
            let _ = std::fs::remove_file(pending);
        }
        if let Some(path) = entry.thumbnail_path {
            let _ = std::fs::remove_file(path);
        }
        self.reset_empty_filter();
        Ok(())
    }

    fn entry_index(&self, file_name: &str) -> std::io::Result<usize> {
        self.entries
            .iter()
            .position(|e| e.file_name == file_name)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "No such recipe in the Recipe Box"))
    }

    fn reset_empty_filter(&mut self) {
        if self.filtered_entries().is_empty() {
            self.selected_category.clear();
        }
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
            paint_jitter: Default::default(),
            paint_seed: 0,
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

        let file = lib.entries[0].file_name.clone();
        let hash = lib.entries[0].hash;
        let bytes = std::fs::read(dir.join(&file)).unwrap();
        let mut second = lib.entries[0].detail.clone();
        second.name = "Antler".into();
        lib.save_part(&dir, second, "Creatures").unwrap();
        assert!(lib.edit_part(&dir, &file, "Antler", "Creatures").is_err());
        lib.edit_part(&dir, &file, "Curved horn", "Creatures").unwrap();
        lib.reorder_part(&dir, 1, 0).unwrap();
        assert_eq!(std::fs::read(dir.join(&file)).unwrap(), bytes);
        let mut reloaded = PartsLibrary::load_all(&dir);
        assert_eq!(reloaded.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Antler", "Curved horn"]);
        assert_eq!(reloaded.entries[1].file_name, file);
        assert_eq!(reloaded.entries[1].hash, hash);
        assert_eq!(reloaded.categories(), vec!["Creatures"]);
        reloaded.delete_part(&dir, &file).unwrap();
        assert!(!dir.join(file).exists());
        assert_eq!(PartsLibrary::load_all(&dir).entries.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
