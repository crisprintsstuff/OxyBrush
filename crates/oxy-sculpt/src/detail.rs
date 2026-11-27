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
/// Where a placed part sits on a surface (RFC 0026): its anchor, the
/// surface normal there, the tangent its local x axis follows (the twist is
/// in it), and its scale. `mirrored` marks a frame reflected through a
/// mirror plane: its bitangent flips, so mapping through it gives the
/// mirror image of the part (left and right swap), not a turned copy.
///
/// INVARIANT: `normal` and `tangent` are unit length and perpendicular.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartFrame {
    pub anchor: Vec3,
    pub normal: Vec3,
    pub tangent: Vec3,
    pub scale: f32,
    pub mirrored: bool,
}

impl PartFrame {
    /// The frame a drop at `anchor` with surface `normal` and `twist`
    /// (radians round the normal) makes: the same one
    /// [`PartDetail::project_to_surface`] uses.
    pub fn from_twist(anchor: Vec3, normal: Vec3, twist: f32, scale: f32) -> Self {
        let n = normal.normalize_or_zero();
        let n = if n.length_squared() < 0.5 { Vec3::Y } else { n };
        let t_base = tangent_for_normal(n);
        let b_base = n.cross(t_base).normalize();
        let (sin_t, cos_t) = twist.sin_cos();
        let tangent = (t_base * cos_t + b_base * sin_t).normalize();
        PartFrame { anchor, normal: n, tangent, scale, mirrored: false }
    }

    /// The frame's y axis in the surface.
    pub fn bitangent(&self) -> Vec3 {
        let b = self.normal.cross(self.tangent).normalize_or_zero();
        if self.mirrored { -b } else { b }
    }

    /// The frame with its tangent made perpendicular to its normal again
    /// (after the normal changed, or values read from a file).
    pub fn orthonormal(mut self) -> Self {
        let n = self.normal.normalize_or_zero();
        self.normal = if n.length_squared() < 0.5 { Vec3::Y } else { n };
        let t = self.tangent - self.normal * self.tangent.dot(self.normal);
        self.tangent = if t.length_squared() < 1e-10 { tangent_for_normal(self.normal) } else { t.normalize() };
        self
    }

    /// A world point for local offset `local` (x tangent, y bitangent, z
    /// normal), in units of `unit` (the detail's reference radius) times the
    /// frame's scale.
    pub fn to_world(&self, local: Vec3, unit: f32) -> Vec3 {
        let o = local * unit * self.scale.max(1e-4);
        self.anchor + self.tangent * o.x + self.bitangent() * o.y + self.normal * o.z
    }

    /// The local offset of world point `p`: the inverse of [`Self::to_world`].
    pub fn to_local(&self, p: Vec3, unit: f32) -> Vec3 {
        let o = (p - self.anchor) / (unit * self.scale.max(1e-4)).max(1e-9);
        Vec3::new(o.dot(self.tangent), o.dot(self.bitangent()), o.dot(self.normal))
    }

    /// A world direction for local direction `d`.
    pub fn dir_to_world(&self, d: Vec3) -> Vec3 {
        self.tangent * d.x + self.bitangent() * d.y + self.normal * d.z
    }

    /// The local direction of world direction `d`.
    pub fn dir_to_local(&self, d: Vec3) -> Vec3 {
        Vec3::new(d.dot(self.tangent), d.dot(self.bitangent()), d.dot(self.normal))
    }

    /// This frame reflected through the plane through `center` with normal
    /// `axis` (unit). Mapping a part through the result gives the mirror
    /// image of mapping it through `self`.
    pub fn reflect(&self, center: Vec3, axis: Vec3) -> Self {
        let point = |p: Vec3| p - 2.0 * (p - center).dot(axis) * axis;
        let dir = |d: Vec3| d - 2.0 * d.dot(axis) * axis;
        PartFrame {
            anchor: point(self.anchor),
            normal: dir(self.normal),
            tangent: dir(self.tangent),
            scale: self.scale,
            mirrored: !self.mirrored,
        }
    }
}

/// Map placed recipes from one frame to another (RFC 0026: Move on
/// surface, and the mirror side of a linked pair). Each sample keeps its
/// place in the part (its local offset in `from`) and is projected onto
/// `mesh` from where `to` puts it, along `to`'s normal, as a drop does.
/// Brush sizes scale by `to.scale / from.scale`. Samples that miss the
/// surface are left out and counted.
///
/// RETURNS: the mapped recipes, in order, and how many samples missed.
pub fn remap_recipes(
    recipes: &[StrokeRecipe],
    from: &PartFrame,
    to: &PartFrame,
    unit: f32,
    mesh: &Mesh,
) -> (Vec<StrokeRecipe>, usize) {
    let ratio = to.scale.max(1e-4) / from.scale.max(1e-4);
    let probe = (unit * to.scale * 4.0).max(0.05);
    let n = to.normal;
    let mut missed = 0;
    let mut place = |p: Vec3| -> Option<(Vec3, Vec3)> {
        let world = to.to_world(from.to_local(p, unit), unit);
        let hit = mesh.raycast(world + n * probe, -n).or_else(|| mesh.raycast(world - n * probe, n));
        match hit {
            Some(h) => Some((h.point, h.normal)),
            None => {
                missed += 1;
                None
            }
        }
    };
    let out = recipes
        .iter()
        .map(|r| {
            let mut mapped = r.clone();
            mapped.settings.radius *= ratio;
            mapped.symmetry = Symmetry::default();
            mapped.events = r
                .events
                .iter()
                .filter_map(|e| match *e {
                    RecipeEvent::Sample { mut sample, spacing, strength, max_dabs } => {
                        let (p, nn) = place(sample.position)?;
                        sample.position = p;
                        sample.normal = nn;
                        Some(RecipeEvent::Sample { sample, spacing, strength, max_dabs })
                    }
                    RecipeEvent::Stamp { mut sample, repeats } => {
                        let (p, nn) = place(sample.position)?;
                        sample.position = p;
                        sample.normal = nn;
                        Some(RecipeEvent::Stamp { sample, repeats })
                    }
                    other => Some(other),
                })
                .collect();
            mapped
        })
        .collect();
    (out, missed)
}

impl PartDetail {
    /// Project this detail onto `mesh` through `frame` (RFC 0026). Like
    /// [`Self::project_to_surface`], but the frame may be mirrored, so the
    /// mirror side of a pair is a true mirror image.
    ///
    /// RETURNS: (mapped recipes, count of samples that missed the surface).
    pub fn project_frame(&self, frame: &PartFrame, mesh: &Mesh) -> (Vec<StrokeRecipe>, usize) {
        // The detail's own coordinates are a frame at the origin.
        let local = PartFrame { anchor: Vec3::ZERO, normal: Vec3::Z, tangent: Vec3::X, scale: 1.0, mirrored: false };
        let unit = self.reference_radius;
        // Local samples are offsets / reference_radius; put them in world
        // units of `local` (scale 1) so `remap_recipes` maps them through.
        let world: Vec<StrokeRecipe> = self
            .recipes
            .iter()
            .map(|r| {
                let mut r = r.clone();
                for e in &mut r.events {
                    if let RecipeEvent::Sample { sample, .. } | RecipeEvent::Stamp { sample, .. } = e {
                        sample.position *= unit;
                    }
                }
                r
            })
            .collect();
        remap_recipes(&world, &local, frame, unit, mesh)
    }

    /// A detail from recipes placed through `frame`: the inverse of
    /// [`Self::project_frame`] (RFC 0026: Save back to Recipe Box). Sizes
    /// go back to the detail's own scale; a mirrored frame's recipes come
    /// back in the frame's handedness, so the saved part keeps the hand it
    /// was placed with.
    pub fn from_placed(
        name: &str,
        recipes: &[StrokeRecipe],
        frame: &PartFrame,
        reference_radius: f32,
    ) -> Result<Self, String> {
        if recipes.is_empty() {
            return Err("There are no strokes to save".into());
        }
        let unit = reference_radius.max(1e-4);
        let mut alphas: Vec<Alpha> = Vec::new();
        let recipes = recipes
            .iter()
            .map(|r| {
                let mut out = r.clone();
                out.symmetry = Symmetry::default();
                out.settings.radius /= frame.scale.max(1e-4);
                if let Some(a) = &out.alpha
                    && !alphas.iter().any(|x| x.name == a.alpha.name)
                {
                    alphas.push((*a.alpha).clone());
                }
                for e in &mut out.events {
                    if let RecipeEvent::Sample { sample, .. } | RecipeEvent::Stamp { sample, .. } = e {
                        sample.position = frame.to_local(sample.position, unit);
                        sample.normal = frame.dir_to_local(sample.normal.normalize_or_zero());
                    }
                }
                out
            })
            .collect();
        Ok(PartDetail { name: name.to_string(), reference_radius: unit, recipes, alphas })
    }
}

fn tangent_for_normal(normal: Vec3) -> Vec3 {
    let axis = if normal.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    normal.cross(axis).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BrushSettings;
    use crate::StrokeSample;

    /// An asymmetric L-shaped detail: a long leg along local x and a
    /// short one along local +y, so a turned copy and a mirror image differ.
    fn l_detail() -> PartDetail {
        let event = |x: f32, y: f32| RecipeEvent::Sample {
            sample: StrokeSample { position: Vec3::new(x, y, 0.0), normal: Vec3::Z, pressure: 1.0 },
            spacing: 0.1,
            strength: 1.0,
            max_dabs: 0,
        };
        let mut recipe = StrokeRecipe {
            settings: BrushSettings { radius: 0.05, ..BrushSettings::default() },
            invert: false,
            symmetry: Symmetry { x: true, ..Default::default() },
            center: Vec3::ZERO,
            view: Vec3::Z,
            grab_through: false,
            paint: Vec3::ONE,
            paint_jitter: Default::default(),
            paint_seed: 0,
            rotate_jitter_seed: 0,
            legacy_alpha: false,
            alpha: None,
            events: vec![event(0.0, 0.0), event(2.0, 0.0), event(4.0, 0.0), event(4.0, 1.5)],
        };
        recipe.settings.radius = 0.05;
        PartDetail { name: "L".into(), reference_radius: 0.05, recipes: vec![recipe], alphas: Vec::new() }
    }

    fn points(recipes: &[StrokeRecipe]) -> Vec<Vec3> {
        recipes
            .iter()
            .flat_map(|r| r.events.iter())
            .filter_map(|e| match e {
                RecipeEvent::Sample { sample, .. } | RecipeEvent::Stamp { sample, .. } => Some(sample.position),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_frame_maps_like_a_drop_and_its_reflection_is_a_mirror_image() {
        let sphere = oxy_mesh::primitives::icosphere(5, 1.0);
        let detail = l_detail();
        let normal = Vec3::new(0.5, 0.6, 0.62).normalize();
        let frame = PartFrame::from_twist(normal, normal, 0.4, 1.5);
        // The same placement as the drop path.
        let (a, missed) = detail.project_frame(&frame, &sphere);
        let (b, _) = detail.project_to_surface(normal, normal, 0.4, 1.5, &sphere);
        assert_eq!(missed, 0);
        for (p, q) in points(&a).iter().zip(points(&b)) {
            assert!(p.distance(q) < 1e-5, "{p} vs {q}");
        }
        assert!((a[0].settings.radius - 0.05 * 1.5).abs() < 1e-6, "brush size scales with the part");
        assert_eq!(a[0].symmetry, Symmetry::default(), "placed recipes don't mirror again");
        // Reflected through x = 0: every point is the mirror of the source's
        // (the sphere is symmetric), so the L's short leg points the other way.
        let mirror = frame.reflect(Vec3::ZERO, Vec3::X);
        assert!(mirror.mirrored);
        let (m, _) = detail.project_frame(&mirror, &sphere);
        for (p, q) in points(&a).iter().zip(points(&m)) {
            let reflected = Vec3::new(-p.x, p.y, p.z);
            assert!(reflected.distance(q) < 2e-3, "mirror of {p} is {reflected}, got {q}");
        }
        // A turned copy (negated twist, the old way) is not that mirror image.
        let turned = PartFrame::from_twist(mirror.anchor, mirror.normal, -0.4, 1.5);
        let (t, _) = detail.project_frame(&turned, &sphere);
        let worst = points(&t).iter().zip(points(&m)).map(|(p, q)| p.distance(q)).fold(0.0f32, f32::max);
        assert!(worst > 0.02, "a turned copy differs from the mirror image ({worst})");
    }

    #[test]
    fn saving_back_through_the_frame_gives_the_detail_back_in_its_own_hand() {
        let sphere = oxy_mesh::primitives::icosphere(5, 1.0);
        let detail = l_detail();
        let normal = Vec3::new(-0.3, 0.8, 0.5).normalize();
        for mirrored in [false, true] {
            let mut frame = PartFrame::from_twist(normal, normal, 1.1, 0.8);
            if mirrored {
                frame = frame.reflect(Vec3::ZERO, Vec3::X);
            }
            let (placed, _) = detail.project_frame(&frame, &sphere);
            let back = PartDetail::from_placed("L", &placed, &frame, detail.reference_radius).unwrap();
            assert!((back.recipes[0].settings.radius - 0.05).abs() < 1e-6, "size back to the detail's own");
            for (p, q) in points(&back.recipes).iter().zip(points(&detail.recipes)) {
                // On a curved surface the projection lifts points a little
                // off the tangent plane; across the plane they match.
                assert!(p.truncate().distance(q.truncate()) < 0.05, "mirrored={mirrored}: {p} vs {q}");
            }
        }
    }

    #[test]
    fn remapping_moves_a_placed_part_and_misses_are_counted() {
        let sphere = oxy_mesh::primitives::icosphere(5, 1.0);
        let detail = l_detail();
        let from = PartFrame::from_twist(Vec3::Y, Vec3::Y, 0.0, 1.0);
        let (placed, _) = detail.project_frame(&from, &sphere);
        let to = PartFrame::from_twist(Vec3::Z, Vec3::Z, 0.0, 2.0);
        let (moved, missed) = remap_recipes(&placed, &from, &to, detail.reference_radius, &sphere);
        assert_eq!(missed, 0);
        let pts = points(&moved);
        assert!(pts[0].distance(Vec3::Z) < 1e-3, "the anchor sample goes to the new anchor: {}", pts[0]);
        assert!(pts.iter().all(|p| (p.length() - 1.0).abs() < 0.02), "on the surface");
        assert!((moved[0].settings.radius - 0.1).abs() < 1e-5, "twice the scale, twice the brush");
        // Far off the mesh, samples miss.
        let away = PartFrame::from_twist(Vec3::new(5.0, 0.0, 0.0), Vec3::Y, 0.0, 1.0);
        let (_, missed) = remap_recipes(&placed, &from, &away, detail.reference_radius, &sphere);
        assert_eq!(missed, 4);
    }

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
            paint_jitter: Default::default(),
            paint_seed: 0,
            rotate_jitter_seed: 0,
            legacy_alpha: false,
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
            paint_jitter: Default::default(),
            paint_seed: 0,
            rotate_jitter_seed: 0,
            legacy_alpha: false,
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
