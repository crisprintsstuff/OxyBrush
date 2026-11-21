//! Portable detail recipes (RFC 0025 Phase 4, RFC 0026: Recipe Box).
//!
//! A part/detail stores stroke recipes relative to an anchor frame (first sample
//! of the first stroke, its surface normal, and initial tangent). Coordinates
//! are dimensionless, scaled by the reference brush radius.

use crate::Symmetry;
use crate::alpha::Alpha;
use crate::recipe::{RecipeEvent, StrokeRecipe};
use glam::Vec3;
use oxy_mesh::Mesh;
use std::sync::Arc;

/// Magic bytes for `.oxydetail` binary format.
pub const DETAIL_MAGIC: &[u8; 4] = b"OXDT";
/// Current file format version.
pub const DETAIL_VERSION: u8 = 1;

/// A portable sculpted detail made of stroke recipes relative to an anchor frame.
#[derive(Clone, Debug, PartialEq)]
pub struct PartDetail {
    pub name: String,
    pub reference_radius: f32,
    pub recipes: Vec<StrokeRecipe>,
    pub alphas: Vec<Alpha>,
}

impl PartDetail {
    /// Create a new detail from recorded stroke recipes.
    ///
    /// - Sets the anchor frame from the first sample of the first stroke.
    /// - Normalizes sample positions relative to the anchor divided by `reference_radius`.
    /// - Clears symmetry on each recipe (reducing symmetric strokes to the positive side).
    pub fn from_recipes(
        name: &str,
        recipes: &[StrokeRecipe],
        piece_center: Vec3,
        symmetry: Symmetry,
    ) -> Result<Self, String> {
        if recipes.is_empty() {
            return Err("Cannot create a part from empty recipes".into());
        }

        // Find the first sample across recipes to establish the anchor frame
        let mut first_sample = None;
        let mut second_sample = None;

        for r in recipes {
            for e in &r.events {
                match e {
                    RecipeEvent::Sample { sample, .. } | RecipeEvent::Stamp { sample, .. } => match &first_sample {
                        None => first_sample = Some(*sample),
                        Some(first) => {
                            if second_sample.is_none() && first.position.distance(sample.position) > 1e-4 {
                                second_sample = Some(*sample);
                                break;
                            }
                        }
                    },
                    RecipeEvent::Modulation { .. } => {}
                }
            }
            if second_sample.is_some() {
                break;
            }
        }

        let first = first_sample.ok_or_else(|| "No stroke samples found in recipes".to_string())?;
        let p0 = first.position;
        let n0 = first.normal.normalize_or_zero();
        let n0 = if n0.length_squared() < 0.5 { Vec3::Y } else { n0 };

        // Determine tangent direction in tangent plane
        let t0 = if let Some(sec) = second_sample {
            let v = sec.position - p0;
            let projected = v - n0 * v.dot(n0);
            if projected.length() > 1e-4 { projected.normalize() } else { tangent_for_normal(n0) }
        } else {
            tangent_for_normal(n0)
        };
        let b0 = n0.cross(t0).normalize();

        let ref_radius = recipes[0].settings.radius.max(1e-4);

        // Convert recipes into anchor-relative coordinates
        let mut normalized_recipes = Vec::with_capacity(recipes.len());
        let mut alphas = Vec::new();

        for r in recipes {
            let mut norm_r = r.clone();
            norm_r.symmetry = Symmetry::default(); // Clear symmetry flags

            if let Some(alpha_stamp) = &norm_r.alpha
                && !alphas.iter().any(|a: &Alpha| a.name == alpha_stamp.alpha.name)
            {
                alphas.push((*alpha_stamp.alpha).clone());
            }

            let mut events = Vec::with_capacity(r.events.len());
            for e in &r.events {
                match *e {
                    RecipeEvent::Sample { mut sample, spacing, strength, max_dabs } => {
                        // Check symmetry filtering: if symmetric, keep only positive side
                        if symmetry.x || symmetry.y || symmetry.z {
                            let rel = sample.position - piece_center;
                            let pos_side = if symmetry.x {
                                rel.x >= -1e-4
                            } else if symmetry.y {
                                rel.y >= -1e-4
                            } else {
                                rel.z >= -1e-4
                            };
                            if !pos_side {
                                continue;
                            }
                        }

                        // Map to anchor frame and normalize by ref_radius
                        let offset = (sample.position - p0) / ref_radius;
                        sample.position = Vec3::new(offset.dot(t0), offset.dot(b0), offset.dot(n0));

                        // Map normal to basis
                        let sn = sample.normal.normalize_or_zero();
                        sample.normal = Vec3::new(sn.dot(t0), sn.dot(b0), sn.dot(n0));

                        events.push(RecipeEvent::Sample { sample, spacing, strength, max_dabs });
                    }
                    RecipeEvent::Stamp { mut sample, repeats } => {
                        if symmetry.x || symmetry.y || symmetry.z {
                            let rel = sample.position - piece_center;
                            let pos_side = if symmetry.x {
                                rel.x >= -1e-4
                            } else if symmetry.y {
                                rel.y >= -1e-4
                            } else {
                                rel.z >= -1e-4
                            };
                            if !pos_side {
                                continue;
                            }
                        }

                        let offset = (sample.position - p0) / ref_radius;
                        sample.position = Vec3::new(offset.dot(t0), offset.dot(b0), offset.dot(n0));

                        let sn = sample.normal.normalize_or_zero();
                        sample.normal = Vec3::new(sn.dot(t0), sn.dot(b0), sn.dot(n0));

                        events.push(RecipeEvent::Stamp { sample, repeats });
                    }
                    RecipeEvent::Modulation { size, strength } => {
                        events.push(RecipeEvent::Modulation { size, strength });
                    }
                }
            }
            norm_r.events = events;
            normalized_recipes.push(norm_r);
        }

        Ok(PartDetail { name: name.to_string(), reference_radius: ref_radius, recipes: normalized_recipes, alphas })
    }

    /// Project this detail onto a target mesh surface at `(anchor, normal)` with `twist` rotation and `scale`.
    ///
    /// RETURNS: (mapped recipes ready to replay on the mesh, count of missed/omitted samples).
    pub fn project_to_surface(
        &self,
        anchor: Vec3,
        normal: Vec3,
        twist: f32,
        scale: f32,
        mesh: &Mesh,
    ) -> (Vec<StrokeRecipe>, usize) {
        let n = normal.normalize_or_zero();
        let n = if n.length_squared() < 0.5 { Vec3::Y } else { n };

        let t_base = tangent_for_normal(n);
        let b_base = n.cross(t_base).normalize();

        // Rotate tangent around normal by twist
        let (sin_t, cos_t) = twist.sin_cos();
        let t = (t_base * cos_t + b_base * sin_t).normalize();
        let b = n.cross(t).normalize();

        let effective_scale = scale.max(1e-4);
        let probe = (self.reference_radius * effective_scale * 4.0).max(0.05);

        let mut out_recipes = Vec::with_capacity(self.recipes.len());
        let mut missed_samples = 0;

        for r in &self.recipes {
            let mut mapped = r.clone();
            mapped.settings.radius *= effective_scale;
            mapped.symmetry = Symmetry::default();

            let mut events = Vec::with_capacity(r.events.len());
            for e in &r.events {
                match *e {
                    RecipeEvent::Sample { mut sample, spacing, strength, max_dabs } => {
                        // Unproject from dimensionless anchor frame to world coordinates
                        let offset = sample.position * self.reference_radius * effective_scale;
                        let world_pos = anchor + t * offset.x + b * offset.y + n * offset.z;

                        // Raycast onto target mesh surface along target normal
                        let hit =
                            mesh.raycast(world_pos + n * probe, -n).or_else(|| mesh.raycast(world_pos - n * probe, n));

                        if let Some(h) = hit {
                            sample.position = h.point;
                            sample.normal = h.normal;
                            events.push(RecipeEvent::Sample { sample, spacing, strength, max_dabs });
                        } else {
                            // Nearest point fallback
                            missed_samples += 1;
                        }
                    }
                    RecipeEvent::Stamp { mut sample, repeats } => {
                        let offset = sample.position * self.reference_radius * effective_scale;
                        let world_pos = anchor + t * offset.x + b * offset.y + n * offset.z;

                        let hit =
                            mesh.raycast(world_pos + n * probe, -n).or_else(|| mesh.raycast(world_pos - n * probe, n));

                        if let Some(h) = hit {
                            sample.position = h.point;
                            sample.normal = h.normal;
                            events.push(RecipeEvent::Stamp { sample, repeats });
                        } else {
                            missed_samples += 1;
                        }
                    }
                    RecipeEvent::Modulation { size, strength } => {
                        events.push(RecipeEvent::Modulation { size, strength });
                    }
                }
            }
            mapped.events = events;
            out_recipes.push(mapped);
        }

        (out_recipes, missed_samples)
    }

    /// Serialize into `.oxydetail` binary format.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(DETAIL_MAGIC);
        b.push(DETAIL_VERSION);

        // Name
        let name_bytes = self.name.as_bytes();
        b.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        b.extend_from_slice(name_bytes);

        // Reference radius
        b.extend_from_slice(&self.reference_radius.to_le_bytes());

        // Alphas count and data
        b.extend_from_slice(&(self.alphas.len() as u32).to_le_bytes());
        for alpha in &self.alphas {
            let aname = alpha.name.as_bytes();
            b.extend_from_slice(&(aname.len() as u32).to_le_bytes());
            b.extend_from_slice(aname);
            b.extend_from_slice(&alpha.size.to_le_bytes());
            b.extend_from_slice(&(alpha.data.len() as u32).to_le_bytes());
            for &s in &alpha.data {
                b.extend_from_slice(&s.to_le_bytes());
            }
        }

        // Recipes count and data
        b.extend_from_slice(&(self.recipes.len() as u32).to_le_bytes());
        for r in &self.recipes {
            let rb = r.to_bytes();
            b.extend_from_slice(&(rb.len() as u32).to_le_bytes());
            b.extend_from_slice(&rb);
        }

        b
    }

    /// Deserialize from `.oxydetail` binary format.
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        if data.len() < 5 {
            return Err("File too short for .oxydetail".into());
        }
        if &data[0..4] != DETAIL_MAGIC {
            return Err("Invalid .oxydetail magic header".into());
        }
        let version = data[4];
        if version != DETAIL_VERSION {
            return Err(format!("Unsupported .oxydetail version {version}"));
        }

        let mut pos = 5;
        let read_u32 = |data: &[u8], pos: &mut usize| -> Result<u32, String> {
            if *pos + 4 > data.len() {
                return Err("Unexpected end of data".into());
            }
            let val = u32::from_le_bytes(data[*pos..*pos + 4].try_into().unwrap());
            *pos += 4;
            Ok(val)
        };

        let read_f32 = |data: &[u8], pos: &mut usize| -> Result<f32, String> {
            if *pos + 4 > data.len() {
                return Err("Unexpected end of data".into());
            }
            let val = f32::from_le_bytes(data[*pos..*pos + 4].try_into().unwrap());
            *pos += 4;
            Ok(val)
        };

        // Name
        let name_len = read_u32(data, &mut pos)? as usize;
        if pos + name_len > data.len() {
            return Err("Unexpected end of name".into());
        }
        let name = String::from_utf8_lossy(&data[pos..pos + name_len]).into_owned();
        pos += name_len;

        // Reference radius
        let reference_radius = read_f32(data, &mut pos)?;

        // Alphas
        let alpha_count = read_u32(data, &mut pos)? as usize;
        let mut alphas = Vec::with_capacity(alpha_count);
        for _ in 0..alpha_count {
            let aname_len = read_u32(data, &mut pos)? as usize;
            if pos + aname_len > data.len() {
                return Err("Unexpected end of alpha name".into());
            }
            let aname = String::from_utf8_lossy(&data[pos..pos + aname_len]).into_owned();
            pos += aname_len;
            let asize = read_u32(data, &mut pos)?;
            let data_len = read_u32(data, &mut pos)? as usize;
            if pos + data_len * 4 > data.len() {
                return Err("Unexpected end of alpha samples".into());
            }
            let mut alpha_data = Vec::with_capacity(data_len);
            for _ in 0..data_len {
                let s = f32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
                pos += 4;
                alpha_data.push(s);
            }
            alphas.push(Alpha { name: aname, size: asize, data: alpha_data });
        }

        // Recipes
        let recipe_count = read_u32(data, &mut pos)? as usize;
        let mut recipes = Vec::with_capacity(recipe_count);

        let find_alpha = |lookup: crate::recipe::AlphaLookup| {
            for a in &alphas {
                if lookup.matches(a) {
                    return Some(Arc::new(a.clone()));
                }
            }
            None
        };

        for _ in 0..recipe_count {
            let rlen = read_u32(data, &mut pos)? as usize;
            if pos + rlen > data.len() {
                return Err("Unexpected end of recipe bytes".into());
            }
            let recipe = StrokeRecipe::from_bytes(&data[pos..pos + rlen], &find_alpha)
                .map_err(|e| format!("Failed to read recipe: {e}"))?;
            pos += rlen;
            recipes.push(recipe);
        }

        Ok(PartDetail { name, reference_radius, recipes, alphas })
    }
}

/// Compute a stable tangent vector perpendicular to `normal`.
fn tangent_for_normal(normal: Vec3) -> Vec3 {
    let axis = if normal.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    normal.cross(axis).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BrushSettings;
    use crate::StrokeSample;

    #[test]
    fn part_detail_binary_round_trip() {
        let mut recipe = StrokeRecipe {
            settings: BrushSettings::default(),
            invert: false,
            symmetry: Symmetry::default(),
            center: Vec3::ZERO,
            view: Vec3::Z,
            grab_through: false,
            paint: Vec3::ONE,
            alpha: None,
            events: vec![RecipeEvent::Sample {
                sample: StrokeSample { position: Vec3::new(1.0, 2.0, 3.0), normal: Vec3::Y, pressure: 0.8 },
                spacing: 0.1,
                strength: 1.0,
                max_dabs: 0,
            }],
        };
        recipe.settings.radius = 0.25;

        let detail =
            PartDetail { name: "TestEar".into(), reference_radius: 0.25, recipes: vec![recipe], alphas: Vec::new() };

        let bytes = detail.to_bytes();
        let loaded = PartDetail::from_bytes(&bytes).expect("reads back cleanly");
        assert_eq!(loaded.name, "TestEar");
        assert!((loaded.reference_radius - 0.25).abs() < 1e-6);
        assert_eq!(loaded.recipes.len(), 1);
        assert_eq!(loaded.recipes[0].events.len(), 1);
    }

    #[test]
    fn part_detail_projection_maps_to_mesh() {
        let sphere = oxy_mesh::primitives::icosphere(3, 1.0);
        let sample = StrokeSample { position: Vec3::new(0.0, 1.0, 0.0), normal: Vec3::Y, pressure: 1.0 };
        let mut recipe = StrokeRecipe {
            settings: BrushSettings::default(),
            invert: false,
            symmetry: Symmetry::default(),
            center: Vec3::ZERO,
            view: Vec3::Z,
            grab_through: false,
            paint: Vec3::ONE,
            alpha: None,
            events: vec![RecipeEvent::Sample { sample, spacing: 0.1, strength: 1.0, max_dabs: 0 }],
        };
        recipe.settings.radius = 0.2;

        let detail = PartDetail::from_recipes("Dot", &[recipe], Vec3::ZERO, Symmetry::default()).expect("makes detail");
        assert_eq!(detail.recipes.len(), 1);

        // Project onto pole of sphere
        let (projected, missed) = detail.project_to_surface(Vec3::new(0.0, 1.0, 0.0), Vec3::Y, 0.0, 1.0, &sphere);
        assert_eq!(missed, 0, "sample projects onto surface");
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].events.len(), 1);
        if let RecipeEvent::Sample { sample: mapped_s, .. } = projected[0].events[0] {
            assert!((mapped_s.position.length() - 1.0).abs() < 0.1, "mapped position lies on sphere");
        } else {
            panic!("expected sample event");
        }
    }
}
