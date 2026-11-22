//! A stroke's recipe: what was drawn, so the stroke can be made again
//! (RFC 0025). Living strokes keep the change a stroke made to each point;
//! the recipe keeps its inputs, which don't depend on the mesh, so the
//! stroke can later be re-sculpted with other settings, along another path
//! or on another surface.
//!
//! [`Stroke`] is deterministic given its inputs, with one exception: the
//! dab budget widens spacing when dabs are slow, so how many dabs a sample
//! placed depends on the machine's speed at the time. The recipe therefore
//! records the plan each sample was applied with, and a replay uses that
//! instead of timing dabs.

use crate::{Alpha, AlphaStamp, Axis, BrushSettings, Stroke, StrokeSample, Symmetry, preset};
use glam::Vec3;
use oxy_mesh::Mesh;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// One input to a stroke, in the order they came.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecipeEvent {
    /// A pointer sample, and the dab plan it was applied with: the
    /// `spacing` and `strength` scales and the most dabs it could place.
    Sample { sample: StrokeSample, spacing: f32, strength: f32, max_dabs: u32 },
    /// A stamp: one dab applied `repeats` times in place.
    Stamp { sample: StrokeSample, repeats: u32 },
    /// Size and strength multipliers for the samples that follow.
    Modulation { size: f32, strength: f32 },
}

/// Everything needed to make a stroke again.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeRecipe {
    pub settings: BrushSettings,
    pub invert: bool,
    pub symmetry: Symmetry,
    /// The point symmetry mirrors and turns about.
    pub center: Vec3,
    /// Camera forward (zero when it wasn't given).
    pub view: Vec3,
    pub grab_through: bool,
    /// Paint brush color (sRGB).
    pub paint: Vec3,
    pub alpha: Option<AlphaStamp>,
    pub events: Vec<RecipeEvent>,
}

/// How a saved recipe names its alpha.
#[derive(Debug, Clone, Copy)]
pub enum AlphaLookup<'a> {
    /// Recipe version 1: the name, the size, and a fingerprint of up to
    /// 4096 raw sample bits. Kept so a project saved by the first recipe
    /// build still finds a built-in alpha.
    Legacy { name: &'a str, size: u32, fingerprint: u64 },
    /// Recipe version 2: the size and a hash of every pixel quantized the
    /// way a project stores it. The name is not an identity.
    Content { size: u32, id: u64 },
}

impl AlphaLookup<'_> {
    /// Whether `alpha` is the one this lookup names.
    pub fn matches(&self, alpha: &Alpha) -> bool {
        match *self {
            AlphaLookup::Legacy { name, size, fingerprint } => {
                alpha.name == name && alpha.size == size && legacy_alpha_fingerprint(alpha) == fingerprint
            }
            AlphaLookup::Content { size, id } => alpha.size == size && alpha_fingerprint(alpha) == id,
        }
    }
}

/// Looks an alpha up from how the recipe named it.
pub type AlphaFinder<'a> = dyn Fn(AlphaLookup<'_>) -> Option<Arc<Alpha>> + 'a;

/// Bytes a saved recipe starts with, and the layout version after them.
const MAGIC: &[u8; 4] = b"OXSR";
/// 1 named an alpha by name plus a sample of raw bits. 2 names it by a
/// hash of every pixel as stored (16-bit), and does not use the name. 3
/// adds the alpha's invert flag; 4 the pressure curves (#305).
const VERSION: u8 = 4;
/// Most events a saved recipe may claim (a sanity bound for damaged files).
const MAX_EVENTS: usize = 4_000_000;

impl StrokeRecipe {
    /// A stroke set up as the recorded one was, before its first sample.
    pub fn stroke(&self) -> Stroke {
        Stroke::new(self.settings, self.symmetry, self.invert)
            .with_view(self.view)
            .with_grab_through(self.grab_through)
            .with_paint_color(self.paint)
            .with_alpha(self.alpha.clone())
            .with_symmetry_center(self.center)
    }

    /// Make the stroke again on `mesh`. `touch(mesh, chunk)` is called
    /// before a chunk is first changed, as for a live stroke.
    pub fn replay(&self, mesh: &mut Mesh, touch: &mut dyn FnMut(&Mesh, u32)) {
        let mut stroke = self.stroke();
        for e in &self.events {
            match *e {
                RecipeEvent::Sample { sample, spacing, strength, max_dabs } => {
                    stroke.add_sample_planned(mesh, sample, Some((spacing, strength, max_dabs)), touch)
                }
                RecipeEvent::Stamp { sample, repeats } => stroke.stamp(mesh, sample, repeats, touch),
                RecipeEvent::Modulation { size, strength } => stroke.set_modulation(size, strength),
            }
        }
    }

    /// Memory the recipe takes, for the living strokes budget (the alpha
    /// image is shared and not counted).
    pub fn approx_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.events.len() * std::mem::size_of::<RecipeEvent>()
    }

    /// The recipe as bytes, for saving.
    ///
    /// ```text
    /// "OXSR"  version:u8 (2; 1 is still read)
    /// brush:text  falloff:text  shape:text          keys as in brush presets
    /// radius strength focal_shift spacing auto_smooth gravity spray
    ///   noise_scale crease_pinch : f32 each
    /// flags:u8      1 pressure_strength, 2 pressure_radius, 4 front_faces_only,
    ///               8 invert, 16 grab_through, 32 has alpha
    /// symmetry:u8   1 x, 2 y, 4 z;  radial:u32  radial_axis:u8 (0 x, 1 y, 2 z)
    /// center:f32*3  view:f32*3  paint:f32*3
    /// [version 4: strength_curve:f32*4 size_curve:f32*4]
    /// [alpha: name:text size:u32 id:u64 rotation:f32 follow:u8 up:f32*3]
    /// events:u32 then per event  tag:u8
    ///   0 sample:   position:f32*3 normal:f32*3 pressure:f32 spacing:f32 strength:f32 max_dabs:u32
    ///   1 stamp:    position:f32*3 normal:f32*3 pressure:f32 repeats:u32
    ///   2 modulate: size:f32 strength:f32
    /// ```
    /// `text` is `len:u32` then UTF-8. The alpha's image isn't stored.
    /// Version 2's `id` is [`alpha_fingerprint`] (every pixel as stored);
    /// the name is kept for the error text and is not part of the identity.
    /// Version 1's `id` is a sample of the raw sample bits, matched with
    /// the name ([`AlphaLookup::Legacy`]).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode(VERSION)
    }

    /// `version` is 1 or 2. Tests write version 1 to check it still reads.
    fn encode(&self, version: u8) -> Vec<u8> {
        let mut b = Vec::with_capacity(96 + self.events.len() * 41);
        b.extend_from_slice(MAGIC);
        b.push(version);
        let s = &self.settings;
        for key in [preset::kind_key(s.kind), preset::falloff_key(s.falloff), preset::shape_key(s.insert_shape)] {
            put_text(&mut b, key);
        }
        for x in [
            s.radius,
            s.strength,
            s.focal_shift,
            s.spacing,
            s.auto_smooth,
            s.gravity,
            s.spray,
            s.noise_scale,
            s.crease_pinch,
        ] {
            put_f32(&mut b, x);
        }
        b.push(
            u8::from(s.pressure_strength)
                | u8::from(s.pressure_radius) << 1
                | u8::from(s.front_faces_only) << 2
                | u8::from(self.invert) << 3
                | u8::from(self.grab_through) << 4
                | u8::from(self.alpha.is_some()) << 5,
        );
        let y = &self.symmetry;
        b.push(u8::from(y.x) | u8::from(y.y) << 1 | u8::from(y.z) << 2);
        b.extend_from_slice(&y.radial.to_le_bytes());
        b.push(match y.radial_axis {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        });
        for v in [self.center, self.view, self.paint] {
            put_vec3(&mut b, v);
        }
        if version >= 4 {
            for x in self.settings.pressure_curves.iter().flatten() {
                put_f32(&mut b, *x);
            }
        }
        if let Some(a) = &self.alpha {
            put_text(&mut b, &a.alpha.name);
            b.extend_from_slice(&a.alpha.size.to_le_bytes());
            let id = if version <= 1 { legacy_alpha_fingerprint(&a.alpha) } else { alpha_fingerprint(&a.alpha) };
            b.extend_from_slice(&id.to_le_bytes());
            put_f32(&mut b, a.rotation);
            b.push(u8::from(a.follow_stroke));
            put_vec3(&mut b, a.up);
            if version >= 3 {
                b.push(u8::from(a.invert));
            }
        }
        b.extend_from_slice(&(self.events.len() as u32).to_le_bytes());
        let put_sample = |b: &mut Vec<u8>, s: &StrokeSample| {
            put_vec3(b, s.position);
            put_vec3(b, s.normal);
            put_f32(b, s.pressure);
        };
        for e in &self.events {
            match e {
                RecipeEvent::Sample { sample, spacing, strength, max_dabs } => {
                    b.push(0);
                    put_sample(&mut b, sample);
                    put_f32(&mut b, *spacing);
                    put_f32(&mut b, *strength);
                    b.extend_from_slice(&max_dabs.to_le_bytes());
                }
                RecipeEvent::Stamp { sample, repeats } => {
                    b.push(1);
                    put_sample(&mut b, sample);
                    b.extend_from_slice(&repeats.to_le_bytes());
                }
                RecipeEvent::Modulation { size, strength } => {
                    b.push(2);
                    put_f32(&mut b, *size);
                    put_f32(&mut b, *strength);
                }
            }
        }
        b
    }

    /// Read a recipe written by [`StrokeRecipe::to_bytes`]. `find_alpha`
    /// looks an alpha up from [`AlphaLookup`].
    ///
    /// RETURNS: an error for damaged bytes, a newer layout, a brush this
    /// version doesn't know, or an alpha that can't be found (the stroke
    /// then simply has no recipe). Version 1 and version 2 are both read.
    pub fn from_bytes(bytes: &[u8], find_alpha: &AlphaFinder) -> Result<StrokeRecipe, String> {
        let mut r = Reader { data: bytes, pos: 0 };
        if r.take(4)? != MAGIC {
            return Err("not a stroke recipe".into());
        }
        let version = r.u8()?;
        if !(1..=VERSION).contains(&version) {
            return Err(format!("stroke recipe version {version} is newer than this version reads"));
        }
        let kind = preset::kind_from_key(&r.text()?).ok_or("unknown brush")?;
        let falloff = preset::falloff_from_key(&r.text()?).ok_or("unknown falloff")?;
        let insert_shape = preset::shape_from_key(&r.text()?).ok_or("unknown insert shape")?;
        let mut f = [0.0f32; 9];
        for x in &mut f {
            *x = r.f32()?;
        }
        let [radius, strength, focal_shift, spacing, auto_smooth, gravity, spray, noise_scale, crease_pinch] = f;
        if radius <= 0.0 || spacing <= 0.0 {
            return Err("invalid brush size or spacing".into());
        }
        let flags = r.u8()?;
        let settings = BrushSettings {
            kind,
            radius,
            strength,
            falloff,
            focal_shift,
            spacing,
            pressure_strength: flags & 1 != 0,
            pressure_radius: flags & 2 != 0,
            front_faces_only: flags & 4 != 0,
            auto_smooth,
            gravity,
            spray,
            noise_scale,
            crease_pinch,
            insert_shape,
            pressure_curves: [crate::LINEAR_PRESSURE_SHAPE; 2],
        };
        let mut settings = settings;
        let sym = r.u8()?;
        let radial = r.u32()?;
        let radial_axis = match r.u8()? {
            0 => Axis::X,
            1 => Axis::Y,
            2 => Axis::Z,
            _ => return Err("invalid symmetry axis".into()),
        };
        let symmetry = Symmetry { x: sym & 1 != 0, y: sym & 2 != 0, z: sym & 4 != 0, radial, radial_axis };
        let (center, view, paint) = (r.vec3()?, r.vec3()?, r.vec3()?);
        // Older recipes saved pressure already shaped: straight curves.
        if version >= 4 {
            for curve in &mut settings.pressure_curves {
                for x in curve.iter_mut() {
                    *x = r.f32()?;
                }
            }
        }
        let alpha = if flags & 32 != 0 {
            let name = r.text()?;
            let size = r.u32()?;
            let fingerprint = r.u64()?;
            let rotation = r.f32()?;
            let follow_stroke = r.u8()? != 0;
            let up = r.vec3()?;
            let invert = if version >= 3 { r.u8()? != 0 } else { false };
            let lookup = if version == 1 {
                AlphaLookup::Legacy { name: &name, size, fingerprint }
            } else {
                AlphaLookup::Content { size, id: fingerprint }
            };
            let alpha = find_alpha(lookup).ok_or_else(|| format!("alpha {name:?} not found"))?;
            Some(AlphaStamp { alpha, rotation, follow_stroke, invert, up })
        } else {
            None
        };
        let n = r.u32()? as usize;
        // The smallest event is 9 bytes.
        if n > MAX_EVENTS || n * 9 > bytes.len() {
            return Err("invalid event count".into());
        }
        let mut events = Vec::with_capacity(n);
        for _ in 0..n {
            let tag = r.u8()?;
            let mut sample = || -> Result<StrokeSample, String> {
                Ok(StrokeSample { position: r.vec3()?, normal: r.vec3()?, pressure: r.f32()? })
            };
            events.push(match tag {
                0 => {
                    let sample = sample()?;
                    let (spacing, strength) = (r.f32()?, r.f32()?);
                    if spacing <= 0.0 {
                        return Err("invalid dab spacing".into());
                    }
                    RecipeEvent::Sample { sample, spacing, strength, max_dabs: r.u32()? }
                }
                1 => RecipeEvent::Stamp { sample: sample()?, repeats: r.u32()? },
                2 => RecipeEvent::Modulation { size: r.f32()?, strength: r.f32()? },
                _ => return Err("unknown event".into()),
            });
        }
        Ok(StrokeRecipe {
            settings,
            invert: flags & 8 != 0,
            symmetry,
            center,
            view,
            grab_through: flags & 16 != 0,
            paint,
            alpha,
            events,
        })
    }

    /// After the brush size or spacing changes, raise each sample's dab cap
    /// by the factor the step shrank, so the new spacing can place the dabs
    /// it asks for. A cap that was unlimited stays unlimited. Call this once
    /// on the edited recipe, from the settings it had before the edit.
    pub fn retarget_plan(&mut self, previous: &BrushSettings) {
        let old_step = (previous.radius * previous.spacing).max(1e-6);
        let new_step = (self.settings.radius * self.settings.spacing).max(1e-6);
        let scale = old_step / new_step;
        if (scale - 1.0).abs() < 1e-3 {
            return;
        }
        for event in &mut self.events {
            if let RecipeEvent::Sample { max_dabs, .. } = event
                && *max_dabs != u32::MAX
            {
                let n = (*max_dabs as f32 * scale).ceil().clamp(1.0, 1_000_000.0);
                *max_dabs = n as u32;
            }
        }
    }
}

/// What a stroke did to the surface: the points it changed, and the full
/// position and color change at each (not scaled by a fade).
#[derive(Clone, Debug, PartialEq)]
pub struct StoredChange {
    pub vertices: Vec<u32>,
    pub deltas: Vec<Vec3>,
    pub colors: Vec<Vec3>,
}

/// A stored change and how much of it is on the surface now (its fade times
/// its group's strength).
#[derive(Clone, Debug)]
pub struct SurfaceChange {
    pub vertices: Vec<u32>,
    pub deltas: Vec<Vec3>,
    pub colors: Vec<Vec3>,
    pub applied: f32,
}

/// A rough upper bound on how long [`StrokeRecipe::replay`] takes, in
/// milliseconds. About half a millisecond a dab, which is what a large mesh
/// costs; a smaller mesh finishes sooner. A sample whose dab cap was
/// unlimited counts as one dab (freehand samples are already close together).
pub fn replay_estimate_ms(recipe: &StrokeRecipe) -> f32 {
    let dabs: u64 = recipe
        .events
        .iter()
        .map(|event| match event {
            RecipeEvent::Sample { max_dabs, .. } => {
                if *max_dabs == u32::MAX {
                    1
                } else {
                    (*max_dabs).clamp(1, 64) as u64
                }
            }
            RecipeEvent::Stamp { repeats, .. } => u64::from((*repeats).max(1)),
            RecipeEvent::Modulation { .. } => 0,
        })
        .sum();
    dabs as f32 * 0.5
}

/// Re-sculpt `recipe` in place of one stroke (RFC 0025).
///
/// `mesh` is the surface as it is now. `stroke` is the change being
/// replaced, and `later` are the strokes made after it, each with the share
/// currently on the surface. Those shares are subtracted, `recipe` is run,
/// and the later shares are added back unchanged. The difference the replay
/// made is the new stored change. `applied` is how much of that new change
/// stays on the surface (1 keeps the whole replay).
///
/// `touch(mesh, chunk)` runs before a chunk is first changed, including the
/// sparse adds, so undo can snapshot the surface from before the edit.
///
/// RETURNS: the new stored change, at full strength.
pub fn resculpt(
    mesh: &mut Mesh,
    stroke: &SurfaceChange,
    later: &[SurfaceChange],
    applied: f32,
    recipe: &StrokeRecipe,
    touch: &mut dyn FnMut(&Mesh, u32),
) -> StoredChange {
    let mut seen = HashSet::new();
    touch_vertices(mesh, &stroke.vertices, touch, &mut seen);
    for share in later {
        touch_vertices(mesh, &share.vertices, touch, &mut seen);
    }
    add_share(mesh, stroke, -stroke.applied);
    for share in later {
        add_share(mesh, share, -share.applied);
    }

    // Chunks as they were after the subtract and before the replay: the
    // surface the new stroke is sculpted on.
    let mut before: HashMap<u32, (Vec<Vec3>, Vec<Vec3>)> = HashMap::new();
    {
        let mut on_touch = |mesh: &Mesh, chunk: u32| {
            touch(mesh, chunk);
            before.entry(chunk).or_insert_with(|| snapshot_chunk(mesh, chunk));
        };
        recipe.replay(mesh, &mut on_touch);
    }

    let painted = mesh.color.len() == mesh.positions.len();
    let mut rows: Vec<(u32, Vec3, Vec3)> = Vec::new();
    for (chunk, (positions, colors)) in &before {
        let range = mesh.chunks[*chunk as usize].vertices.clone();
        for (k, v) in range.enumerate() {
            let delta = mesh.positions[v as usize] - positions[k];
            let color = if painted && colors.len() == positions.len() {
                mesh.color[v as usize] - colors[k]
            } else {
                Vec3::ZERO
            };
            if delta != Vec3::ZERO || color != Vec3::ZERO {
                rows.push((v, delta, color));
            }
        }
    }
    rows.sort_unstable_by_key(|row| row.0);

    // A fade other than 1 keeps that share of the new stroke, written from
    // the pre-replay positions so it doesn't depend on adding the change twice.
    if (applied - 1.0).abs() > 1e-5 {
        let by_vertex: HashMap<u32, (Vec3, Vec3)> = rows.iter().map(|row| (row.0, (row.1, row.2))).collect();
        for (chunk, (positions, colors)) in &before {
            let range = mesh.chunks[*chunk as usize].vertices.clone();
            for (k, v) in range.enumerate() {
                let (delta, color) = by_vertex.get(&v).copied().unwrap_or((Vec3::ZERO, Vec3::ZERO));
                mesh.positions[v as usize] = positions[k] + delta * applied;
                if painted && colors.len() == positions.len() {
                    mesh.color[v as usize] = (colors[k] + color * applied).clamp(Vec3::ZERO, Vec3::ONE);
                }
            }
        }
    }

    for share in later {
        add_share(mesh, share, share.applied);
    }

    let mut moved = stroke.vertices.clone();
    moved.extend(rows.iter().map(|row| row.0));
    for share in later {
        moved.extend_from_slice(&share.vertices);
    }
    moved.sort_unstable();
    moved.dedup();
    mesh.update_after_move(&moved);
    if rows.iter().any(|row| row.2 != Vec3::ZERO) {
        mesh.mark_all_dirty();
    }
    mesh.tighten_bounds();

    let any_color = rows.iter().any(|row| row.2 != Vec3::ZERO);
    StoredChange {
        vertices: rows.iter().map(|row| row.0).collect(),
        deltas: rows.iter().map(|row| row.1).collect(),
        colors: if any_color { rows.iter().map(|row| row.2).collect() } else { Vec::new() },
    }
}

fn touch_vertices(mesh: &Mesh, vertices: &[u32], touch: &mut dyn FnMut(&Mesh, u32), seen: &mut HashSet<u32>) {
    for &v in vertices {
        if let Some(&chunk) = mesh.vertex_chunk_of(v)
            && seen.insert(chunk)
        {
            touch(mesh, chunk);
        }
    }
}

fn snapshot_chunk(mesh: &Mesh, chunk: u32) -> (Vec<Vec3>, Vec<Vec3>) {
    let range = mesh.chunks[chunk as usize].vertices.clone();
    let range = range.start as usize..range.end as usize;
    let colors = if mesh.color.len() == mesh.positions.len() { mesh.color[range.clone()].to_vec() } else { Vec::new() };
    (mesh.positions[range].to_vec(), colors)
}

fn add_share(mesh: &mut Mesh, share: &SurfaceChange, scale: f32) {
    if scale == 0.0 {
        return;
    }
    for (j, &v) in share.vertices.iter().enumerate() {
        let index = v as usize;
        if index >= mesh.positions.len() {
            continue;
        }
        mesh.positions[index] += share.deltas[j] * scale;
        if let Some(color) = share.colors.get(j)
            && index < mesh.color.len()
        {
            mesh.color[index] = (mesh.color[index] + *color * scale).clamp(Vec3::ZERO, Vec3::ONE);
        }
    }
}

/// Identity of an alpha by the pixels a project actually stores: FNV-1a
/// over every value quantized to 16-bit, then the size. The name is not
/// included. A captured or imported alpha still matches after save and
/// open, because those files store the same 16-bit values and read them
/// back with `u16 as f32 / 65535`.
pub fn alpha_fingerprint(alpha: &Alpha) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &v in &alpha.data {
        for byte in Alpha::quantize_u16(v).to_le_bytes() {
            h ^= u64::from(byte);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h ^= u64::from(alpha.size);
    h.wrapping_mul(0x0000_0100_0000_01b3)
}

/// Recipe version 1's fingerprint: FNV-1a over up to 4096 evenly spaced
/// raw sample bits. It does not survive the 16-bit save of a custom alpha,
/// which is why version 2 replaced it. Built-in alphas are regenerated, so
/// a version 1 recipe of one of those still matches.
fn legacy_alpha_fingerprint(alpha: &Alpha) -> u64 {
    let step = (alpha.data.len() / 4096).max(1);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in alpha.data.iter().step_by(step) {
        for byte in v.to_bits().to_le_bytes() {
            h ^= u64::from(byte);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn put_f32(b: &mut Vec<u8>, x: f32) {
    b.extend_from_slice(&x.to_le_bytes());
}

fn put_vec3(b: &mut Vec<u8>, v: Vec3) {
    v.to_array().into_iter().for_each(|x| put_f32(b, x));
}

fn put_text(b: &mut Vec<u8>, s: &str) {
    b.extend_from_slice(&(s.len() as u32).to_le_bytes());
    b.extend_from_slice(s.as_bytes());
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len()).ok_or("stroke recipe is cut short")?;
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("four bytes")))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("eight bytes")))
    }

    /// A finite number.
    fn f32(&mut self) -> Result<f32, String> {
        let x = f32::from_le_bytes(self.take(4)?.try_into().expect("four bytes"));
        if x.is_finite() { Ok(x) } else { Err("stroke recipe has a value that isn't a number".into()) }
    }

    fn vec3(&mut self) -> Result<Vec3, String> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }

    fn text(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "stroke recipe text is not UTF-8".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BrushKind, BuiltinAlpha};
    use oxy_mesh::primitives;

    /// A wavy path of samples over the top of a unit sphere.
    fn path(n: usize) -> Vec<StrokeSample> {
        (0..n)
            .map(|i| {
                let t = i as f32 / (n - 1) as f32;
                let p = Vec3::new(-0.5 + t, 1.0, 0.25 * (t * 9.0).sin()).normalize();
                StrokeSample { position: p, normal: p, pressure: 0.4 + 0.6 * t }
            })
            .collect()
    }

    fn sculpt(stroke: &mut Stroke, mesh: &mut Mesh, samples: &[StrokeSample]) {
        for &s in samples {
            stroke.add_sample(mesh, s, &mut |_, _| {});
        }
    }

    #[test]
    fn replaying_a_recipe_makes_the_same_stroke() {
        let base = primitives::icosphere(5, 1.0);
        let samples = path(40);
        for kind in [
            BrushKind::Draw,
            BrushKind::Clay,
            BrushKind::ClayStrips,
            BrushKind::Smooth,
            BrushKind::Inflate,
            BrushKind::Crease,
            BrushKind::Layer,
            BrushKind::Noise,
            BrushKind::Grab,
            BrushKind::SnakeHook,
            BrushKind::KelvinGrab,
        ] {
            let settings = BrushSettings {
                kind,
                radius: 0.18,
                strength: 0.6,
                spray: 0.3,
                gravity: 0.4,
                auto_smooth: 0.2,
                pressure_radius: true,
                pressure_curves: [[0.6, 0.1, 0.8, 0.4], [0.2, 0.7, 0.4, 0.95]],
                ..Default::default()
            };
            let symmetry = Symmetry { x: true, ..Default::default() };
            let alpha = AlphaStamp {
                alpha: Arc::new(BuiltinAlpha::Scales.generate(64)),
                rotation: 0.3,
                follow_stroke: true,
                invert: false,
                up: Vec3::Y,
            };
            let mut live = base.clone();
            // A budget, as the app uses: its plan depends on how fast the
            // dabs ran here, which the recipe must carry.
            let mut stroke = Stroke::new(settings, symmetry, false)
                .with_view(Vec3::NEG_Y)
                .with_alpha(Some(alpha))
                .with_budget(0.05)
                .with_recording();
            stroke.set_modulation(1.2, 0.8);
            sculpt(&mut stroke, &mut live, &samples);
            let recipe = stroke.take_recipe().expect("recorded");
            assert!(stroke.take_recipe().is_none(), "taken once");
            assert_ne!(live.positions, base.positions, "{kind:?} did something");

            let mut again = base.clone();
            let mut touched = Vec::new();
            recipe.replay(&mut again, &mut |_, c| touched.push(c));
            assert!(!touched.is_empty());
            assert_eq!(again.positions, live.positions, "{kind:?} replays exactly");

            // And after a trip through bytes.
            let bytes = recipe.to_bytes();
            let find = |q: AlphaLookup| {
                let a = BuiltinAlpha::Scales.generate(64);
                q.matches(&a).then(|| Arc::new(a))
            };
            let loaded = StrokeRecipe::from_bytes(&bytes, &find).expect("reads back");
            assert_eq!(loaded.events, recipe.events);
            assert_eq!(loaded.settings, recipe.settings);
            let mut third = base.clone();
            loaded.replay(&mut third, &mut |_, _| {});
            assert_eq!(third.positions, live.positions, "{kind:?} replays exactly from bytes");
            // A version 3 recipe (before the curves) reads with straight ones.
            let old = StrokeRecipe::from_bytes(&recipe.encode(3), &find).expect("reads back");
            assert_eq!(old.settings.pressure_curves, [crate::LINEAR_PRESSURE_SHAPE; 2]);
            assert_eq!(old.events, recipe.events);
        }
    }

    #[test]
    fn stamps_replay_and_a_rebuilt_mesh_ends_the_recording() {
        let base = primitives::icosphere(4, 1.0);
        let s = path(3)[1];
        let mut live = base.clone();
        let mut stroke = Stroke::new(BrushSettings::default(), Symmetry::default(), true).with_recording();
        stroke.stamp(&mut live, s, 6, &mut |_, _| {});
        let recipe = stroke.take_recipe().unwrap();
        assert_eq!(recipe.events, vec![RecipeEvent::Stamp { sample: s, repeats: 6 }]);
        let mut again = base.clone();
        recipe.replay(&mut again, &mut |_, _| {});
        assert_eq!(again.positions, live.positions);
        // Dynamic detail rebuilt the mesh mid-stroke: no recipe.
        let mut stroke = Stroke::new(BrushSettings::default(), Symmetry::default(), false).with_recording();
        stroke.add_sample(&mut live, s, &mut |_, _| {});
        stroke.remap_vertices(&[], 0);
        assert!(stroke.take_recipe().is_none());
        // Not recording: none either.
        assert!(Stroke::new(BrushSettings::default(), Symmetry::default(), false).take_recipe().is_none());
    }

    #[test]
    fn damaged_recipes_are_refused() {
        let recipe = StrokeRecipe {
            settings: BrushSettings::default(),
            invert: false,
            symmetry: Symmetry::default(),
            center: Vec3::ZERO,
            view: Vec3::ZERO,
            grab_through: false,
            paint: Vec3::ONE,
            alpha: None,
            events: vec![RecipeEvent::Modulation { size: 1.0, strength: 1.0 }],
        };
        let bytes = recipe.to_bytes();
        let none = |_: AlphaLookup| None;
        assert!(StrokeRecipe::from_bytes(&bytes, &none).is_ok());
        assert!(StrokeRecipe::from_bytes(&bytes[..bytes.len() - 3], &none).is_err(), "cut short");
        let mut newer = bytes.clone();
        newer[4] = 9;
        assert!(StrokeRecipe::from_bytes(&newer, &none).is_err(), "a newer layout");
        let mut junk = bytes.clone();
        junk[0] = b'X';
        assert!(StrokeRecipe::from_bytes(&junk, &none).is_err());
        // An alpha that can't be found: no recipe.
        let with_alpha = StrokeRecipe {
            alpha: Some(AlphaStamp {
                alpha: Arc::new(BuiltinAlpha::Noise.generate(32)),
                rotation: 0.0,
                follow_stroke: false,
                invert: false,
                up: Vec3::Y,
            }),
            ..recipe
        };
        assert!(StrokeRecipe::from_bytes(&with_alpha.to_bytes(), &none).is_err());
    }

    /// Replay `stroke` (already recorded, samples not yet added) over `samples`
    /// and check a second run from the recipe, and from its bytes, matches.
    fn assert_replays(mut stroke: Stroke, base: &Mesh, samples: &[StrokeSample]) {
        let mut live = base.clone();
        sculpt(&mut stroke, &mut live, samples);
        let recipe = stroke.take_recipe().expect("recorded");
        let mut again = base.clone();
        recipe.replay(&mut again, &mut |_, _| {});
        assert_eq!(again.positions, live.positions);
        assert_eq!(again.color, live.color);
        assert_eq!(again.mask, live.mask);
        let find = |_: AlphaLookup| None;
        let loaded = StrokeRecipe::from_bytes(&recipe.to_bytes(), &find).expect("reads back");
        let mut third = base.clone();
        loaded.replay(&mut third, &mut |_, _| {});
        assert_eq!(third.positions, live.positions);
        assert_eq!(third.color, live.color);
        assert_eq!(third.mask, live.mask);
    }

    #[test]
    fn a_widened_spacing_scale_is_what_gets_replayed() {
        let base = primitives::icosphere(5, 1.0);
        let settings =
            BrushSettings { radius: 0.25, spacing: 0.1, strength: 0.8, pressure_strength: false, ..Default::default() };
        let a = StrokeSample { position: Vec3::Y, normal: Vec3::Y, pressure: 1.0 };
        let b = StrokeSample { position: Vec3::new(0.55, 0.8, 0.0).normalize(), normal: Vec3::Y, pressure: 1.0 };
        let run = |force_slow: bool| {
            let mut mesh = base.clone();
            let mut stroke = Stroke::new(settings, Symmetry::default(), false).with_recording();
            if force_slow {
                stroke = stroke.with_budget(1.0);
                stroke.assume_dab_ms(50.0);
            }
            sculpt(&mut stroke, &mut mesh, &[a, b]);
            (mesh, stroke.dab_count(), stroke.take_recipe().expect("recorded"))
        };
        let (wide_mesh, wide_dabs, recipe) = run(true);
        let (plain_mesh, plain_dabs, _) = run(false);
        let scales: Vec<f32> = recipe
            .events
            .iter()
            .filter_map(|e| match e {
                RecipeEvent::Sample { spacing, .. } => Some(*spacing),
                _ => None,
            })
            .collect();
        assert!(scales.iter().any(|k| *k > 1.0), "spacing was widened: {scales:?}");
        assert!(wide_dabs < plain_dabs, "widening placed fewer dabs: {wide_dabs} vs {plain_dabs}");
        assert_ne!(wide_mesh.positions, plain_mesh.positions, "widening changed the stroke");
        let mut again = base.clone();
        recipe.replay(&mut again, &mut |_, _| {});
        assert_eq!(again.positions, wide_mesh.positions);
        let loaded = StrokeRecipe::from_bytes(&recipe.to_bytes(), &|_: AlphaLookup| None).unwrap();
        let mut third = base.clone();
        loaded.replay(&mut third, &mut |_, _| {});
        assert_eq!(third.positions, wide_mesh.positions);
    }

    #[test]
    fn paint_mask_symmetry_and_grab_brushes_replay() {
        let base = primitives::icosphere(4, 1.0);
        let samples = path(12);
        // Paint changes color and not position.
        let paint = Stroke::new(
            BrushSettings { kind: BrushKind::Paint, radius: 0.3, strength: 1.0, ..Default::default() },
            Symmetry::default(),
            false,
        )
        .with_paint_color(Vec3::new(0.15, 0.4, 0.85))
        .with_recording();
        let mut painted = base.clone();
        let mut stroke = paint;
        sculpt(&mut stroke, &mut painted, &samples);
        assert!(painted.color.iter().any(|c| c.x < 0.5), "the color landed");
        assert_eq!(painted.positions, base.positions, "paint does not move the surface");
        let recipe = stroke.take_recipe().unwrap();
        assert_eq!(recipe.paint, Vec3::new(0.15, 0.4, 0.85));
        let mut again = base.clone();
        recipe.replay(&mut again, &mut |_, _| {});
        assert_eq!(again.color, painted.color);
        assert_eq!(again.positions, painted.positions);

        // A mask holds the masked side still, and the mask itself is unchanged.
        let mut masked = base.clone();
        for (v, m) in masked.mask.iter_mut().enumerate() {
            if masked.positions[v].x > 0.0 {
                *m = 1.0;
            }
        }
        let mask_before = masked.mask.clone();
        let mut held = masked.clone();
        let mut stroke = Stroke::new(
            BrushSettings { radius: 0.35, strength: 1.0, ..Default::default() },
            Symmetry::default(),
            false,
        )
        .with_recording();
        sculpt(&mut stroke, &mut held, &samples);
        assert_eq!(held.mask, mask_before, "a Draw stroke does not change the mask");
        assert!(
            mask_before.iter().enumerate().all(|(v, m)| *m < 1.0 || held.positions[v] == masked.positions[v]),
            "fully masked points stay put"
        );
        let recipe = stroke.take_recipe().unwrap();
        let mut again = masked.clone();
        recipe.replay(&mut again, &mut |_, _| {});
        assert_eq!(again.positions, held.positions);
        assert_eq!(again.mask, held.mask);
        let _ = mask_before;

        // The Mask brush changes the mask.
        assert_replays(
            Stroke::new(
                BrushSettings { kind: BrushKind::Mask, radius: 0.3, strength: 1.0, ..Default::default() },
                Symmetry::default(),
                false,
            )
            .with_recording(),
            &base,
            &samples,
        );

        // Mirror about a centre that is not the origin.
        let center = Vec3::new(2.5, -0.4, 0.7);
        let mut data = base.to_data();
        for p in &mut data.positions {
            *p += center;
        }
        let off = Mesh::from_data(data);
        let off_samples: Vec<StrokeSample> = samples
            .iter()
            .map(|s| StrokeSample { position: s.position + center, normal: s.normal, pressure: s.pressure })
            .collect();
        let mut live = off.clone();
        let mut stroke = Stroke::new(
            BrushSettings { radius: 0.3, strength: 0.8, ..Default::default() },
            Symmetry { x: true, ..Default::default() },
            false,
        )
        .with_symmetry_center(center)
        .with_recording();
        sculpt(&mut stroke, &mut live, &off_samples);
        let recipe = stroke.take_recipe().unwrap();
        assert_eq!(recipe.center, center);
        let moved = |side: f32| {
            live.positions
                .iter()
                .zip(&off.positions)
                .any(|(p, o)| (p.x - center.x) * side > 0.05 && p.distance(*o) > 1e-4)
        };
        assert!(moved(1.0) && moved(-1.0), "both sides of the off-centre mirror moved");
        let mut again = off.clone();
        recipe.replay(&mut again, &mut |_, _| {});
        assert_eq!(again.positions, live.positions);

        // Radial copies around Y.
        let mut live = base.clone();
        let mut stroke = Stroke::new(
            BrushSettings { radius: 0.22, strength: 0.9, ..Default::default() },
            Symmetry { radial: 6, radial_axis: Axis::Y, ..Default::default() },
            false,
        )
        .with_recording();
        sculpt(&mut stroke, &mut live, &samples);
        let recipe = stroke.take_recipe().unwrap();
        assert_eq!(recipe.symmetry.radial, 6);
        let mut plain = base.clone();
        let mut one = Stroke::new(
            BrushSettings { radius: 0.22, strength: 0.9, ..Default::default() },
            Symmetry::default(),
            false,
        );
        sculpt(&mut one, &mut plain, &samples);
        let moved_points = |m: &Mesh, from: &Mesh| -> usize {
            (0..m.positions.len()).filter(|&i| m.positions[i].distance(from.positions[i]) > 1e-5).count()
        };
        assert!(moved_points(&live, &base) > moved_points(&plain, &base), "radial copies reach more of the surface");
        let mut again = base.clone();
        recipe.replay(&mut again, &mut |_, _| {});
        assert_eq!(again.positions, live.positions);

        // Grab-like brushes: the drag between samples is the stroke.
        let drag = [
            StrokeSample { position: Vec3::Y, normal: Vec3::Y, pressure: 1.0 },
            StrokeSample { position: Vec3::new(0.25, 0.95, 0.05).normalize(), normal: Vec3::Y, pressure: 1.0 },
        ];
        for kind in [BrushKind::TopoMove, BrushKind::KelvinTwist, BrushKind::KelvinScale] {
            let mut live = base.clone();
            let mut stroke = Stroke::new(
                BrushSettings { kind, radius: 0.4, strength: 1.0, ..Default::default() },
                Symmetry::default(),
                false,
            )
            .with_view(Vec3::NEG_Z)
            .with_recording();
            sculpt(&mut stroke, &mut live, &drag);
            assert_ne!(live.positions, base.positions, "{kind:?} moved the surface");
            let recipe = stroke.take_recipe().unwrap();
            let mut again = base.clone();
            recipe.replay(&mut again, &mut |_, _| {});
            assert_eq!(again.positions, live.positions, "{kind:?} replays exactly");
        }
    }

    #[test]
    fn a_saved_alpha_matches_after_quantizing_and_renaming() {
        let mut raw = vec![0.1f32; 64];
        raw[0] = 0.1234567;
        raw[9] = 0.987654;
        raw[40] = 0.5;
        let original = Alpha { name: "Capture 1".into(), size: 8, data: raw.clone() };
        let id = alpha_fingerprint(&original);
        let mut renamed = original.clone();
        renamed.name = "Not the saved name".into();
        assert_eq!(alpha_fingerprint(&renamed), id, "the name is not part of the identity");
        // What opening the project does: store u16, read back as f32.
        let stored: Vec<f32> = raw.iter().map(|&v| Alpha::quantize_u16(v) as f32 / 65535.0).collect();
        let loaded_alpha = Alpha { name: "Capture 1".into(), size: 8, data: stored };
        assert_eq!(alpha_fingerprint(&loaded_alpha), id, "the stored pixels still match");
        assert_ne!(
            legacy_alpha_fingerprint(&original),
            legacy_alpha_fingerprint(&loaded_alpha),
            "the old raw-bit fingerprint does not survive the save"
        );
        // Every stored code round-trips, so the hash can't drift on open.
        for q in 0u32..=65535 {
            let v = q as f32 / 65535.0;
            assert_eq!(u32::from(Alpha::quantize_u16(v)), q);
        }
        let recipe = StrokeRecipe {
            settings: BrushSettings::default(),
            invert: false,
            symmetry: Symmetry::default(),
            center: Vec3::ZERO,
            view: Vec3::ZERO,
            grab_through: false,
            paint: Vec3::ONE,
            alpha: Some(AlphaStamp {
                alpha: Arc::new(original.clone()),
                rotation: 0.2,
                follow_stroke: true,
                invert: true,
                up: Vec3::Y,
            }),
            events: vec![],
        };
        let found = Arc::new(loaded_alpha);
        let find = |q: AlphaLookup| q.matches(&found).then(|| found.clone());
        let back = StrokeRecipe::from_bytes(&recipe.to_bytes(), &find).expect("version 3 finds the renamed pixels");
        assert!(back.alpha.as_ref().is_some_and(|a| a.invert));
        // Version 2 encodes without invert byte and decodes with invert = false.
        let v2 = recipe.encode(2);
        let back_v2 = StrokeRecipe::from_bytes(&v2, &find).expect("version 2 reads");
        assert_eq!(back_v2.alpha.as_ref().map(|a| a.invert), Some(false));
        // Version 1 still reads, and still needs the name plus the old fingerprint.
        let v1 = recipe.encode(1);
        assert_eq!(v1[4], 1);
        assert!(StrokeRecipe::from_bytes(&v1, &find).is_err(), "version 1 does not match the quantized image");
        let exact = Arc::new(original);
        let find_v1 = |q: AlphaLookup| q.matches(&exact).then(|| exact.clone());
        assert!(StrokeRecipe::from_bytes(&v1, &find_v1).is_ok(), "version 1 still reads the original image");
    }

    /// Positions after a stroke, minus the positions before it, for the
    /// points that moved.
    fn stored(before: &Mesh, after: &Mesh) -> SurfaceChange {
        let mut rows: Vec<(u32, Vec3)> = before
            .positions
            .iter()
            .zip(&after.positions)
            .enumerate()
            .filter(|(_, (a, b))| *a != *b)
            .map(|(i, (a, b))| (i as u32, *b - *a))
            .collect();
        rows.sort_unstable_by_key(|row| row.0);
        SurfaceChange {
            vertices: rows.iter().map(|row| row.0).collect(),
            deltas: rows.iter().map(|row| row.1).collect(),
            colors: Vec::new(),
            applied: 1.0,
        }
    }

    #[test]
    fn resculpting_one_stroke_leaves_the_later_stroke_stored() {
        let base = primitives::icosphere(4, 1.0);
        let samples = path(24);
        let settings = BrushSettings { radius: 0.22, strength: 0.8, ..Default::default() };
        let mut mesh = base.clone();
        let mut stroke = Stroke::new(settings, Symmetry::default(), false).with_recording();
        sculpt(&mut stroke, &mut mesh, &samples);
        let recipe = stroke.take_recipe().unwrap();
        let first = stored(&base, &mesh);
        let after_first = mesh.clone();

        let later_samples: Vec<StrokeSample> = samples
            .iter()
            .map(|sample| StrokeSample {
                position: (sample.position + Vec3::new(0.0, 0.05, 0.2)).normalize(),
                normal: (sample.position + Vec3::new(0.0, 0.05, 0.2)).normalize(),
                pressure: sample.pressure,
            })
            .collect();
        let mut second = Stroke::new(settings, Symmetry::default(), false);
        sculpt(&mut second, &mut mesh, &later_samples);
        let later = stored(&after_first, &mesh);
        assert!(!later.vertices.is_empty());

        let mut edited = recipe.clone();
        edited.settings.radius = 0.4;
        edited.settings.strength = 0.3;
        edited.retarget_plan(&recipe.settings);
        let mut peeled = mesh.clone();
        super::add_share(&mut peeled, &first, -1.0);
        super::add_share(&mut peeled, &later, -1.0);
        let peel = peeled.positions.iter().zip(&base.positions).map(|(a, b)| (*a - *b).length()).fold(0.0f32, f32::max);
        let mut again = mesh.clone();
        let new = resculpt(&mut again, &first, std::slice::from_ref(&later), 1.0, &edited, &mut |_, _| {});
        assert_ne!(new.deltas, first.deltas);

        assert!(peel < 1e-5, "taking the strokes off returns the surface they were drawn on (worst {peel})");
        // Replay on that surface, not on a saved copy: a few ulps in the base
        // can move a vertex across a dab's edge.
        let mut fresh = peeled;
        edited.replay(&mut fresh, &mut |_, _| {});
        super::add_share(&mut fresh, &later, later.applied);
        fresh.update_after_move(&new.vertices);
        for (a, b) in fresh.positions.iter().zip(&again.positions) {
            assert!((*a - *b).length() < 1e-5, "re-sculpt matches a fresh replay plus the later stroke");
        }
    }
}
