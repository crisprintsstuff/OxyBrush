//! Stroke engine and brush kernels.
//!
//! A stroke is a sequence of samples (surface hits from the pointer). The
//! engine spaces them into *dabs*, mirrors each dab for symmetry, and applies
//! the active brush kernel to every vertex inside the dab's sphere. Work is
//! parallelized per chunk; before a chunk is modified the caller's `touch`
//! callback runs so undo can snapshot it.

mod falloff;

pub use falloff::Falloff;

use glam::Vec3;
use oxy_mesh::Mesh;
use rayon::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrushKind {
    Draw,
    Clay,
    Smooth,
    Inflate,
    Flatten,
    Grab,
    Mask,
}

impl BrushKind {
    pub const ALL: [BrushKind; 7] = [
        BrushKind::Draw,
        BrushKind::Clay,
        BrushKind::Smooth,
        BrushKind::Inflate,
        BrushKind::Flatten,
        BrushKind::Grab,
        BrushKind::Mask,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BrushKind::Draw => "Draw",
            BrushKind::Clay => "Clay",
            BrushKind::Smooth => "Smooth",
            BrushKind::Inflate => "Inflate",
            BrushKind::Flatten => "Flatten",
            BrushKind::Grab => "Grab",
            BrushKind::Mask => "Mask",
        }
    }

    /// Default strength that feels balanced for each kernel.
    pub fn default_strength(self) -> f32 {
        match self {
            BrushKind::Smooth => 0.5,
            BrushKind::Grab => 1.0,
            BrushKind::Mask => 0.8,
            _ => 0.4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrushSettings {
    pub kind: BrushKind,
    /// World-space radius of the brush.
    pub radius: f32,
    /// 0..=1
    pub strength: f32,
    pub falloff: Falloff,
    /// Distance between dabs as a fraction of the radius.
    pub spacing: f32,
    /// Pressure affects strength.
    pub pressure_strength: bool,
    /// Pressure affects radius.
    pub pressure_radius: bool,
    /// Skip vertices facing away from the dab normal (protects thin parts).
    pub front_faces_only: bool,
    /// Extra smoothing applied after each dab, 0..=1.
    pub auto_smooth: f32,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            kind: BrushKind::Draw,
            radius: 0.2,
            strength: BrushKind::Draw.default_strength(),
            falloff: Falloff::Smooth,
            spacing: 0.15,
            pressure_strength: true,
            pressure_radius: false,
            front_faces_only: true,
            auto_smooth: 0.0,
        }
    }
}

/// Mirror planes through the object origin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Symmetry {
    pub x: bool,
    pub y: bool,
    pub z: bool,
}

impl Symmetry {
    /// All mirror sign combinations, the identity first.
    pub fn mirrors(&self) -> Vec<Vec3> {
        let mut out = vec![Vec3::ONE];
        for (on, axis) in [
            (self.x, Vec3::new(-1.0, 1.0, 1.0)),
            (self.y, Vec3::new(1.0, -1.0, 1.0)),
            (self.z, Vec3::new(1.0, 1.0, -1.0)),
        ] {
            if on {
                let n = out.len();
                for i in 0..n {
                    out.push(out[i] * axis);
                }
            }
        }
        out
    }
}

/// One pointer sample along a stroke.
#[derive(Debug, Clone, Copy)]
pub struct StrokeSample {
    /// Surface hit point, or (for Grab) the point on the drag plane.
    pub position: Vec3,
    /// Surface normal at the hit.
    pub normal: Vec3,
    /// Pen pressure 0..=1 (1.0 for mice).
    pub pressure: f32,
}

#[derive(Debug, Clone, Copy)]
struct Dab {
    center: Vec3,
    normal: Vec3,
    radius: f32,
    strength: f32,
}

/// (vertex, weight, original position)
type GrabVertex = (u32, f32, Vec3);

struct GrabState {
    origin: Vec3,
    /// Captured vertices per mirror sign.
    captured: Vec<(Vec3, Vec<GrabVertex>)>,
}

/// An in-progress stroke.
pub struct Stroke {
    settings: BrushSettings,
    mirrors: Vec<Vec3>,
    invert: bool,
    last: Option<StrokeSample>,
    carry: f32,
    grab: Option<GrabState>,
    dab_count: u32,
}

impl Stroke {
    pub fn new(settings: BrushSettings, symmetry: Symmetry, invert: bool) -> Self {
        Self { settings, mirrors: symmetry.mirrors(), invert, last: None, carry: 0.0, grab: None, dab_count: 0 }
    }

    pub fn settings(&self) -> &BrushSettings {
        &self.settings
    }

    pub fn dab_count(&self) -> u32 {
        self.dab_count
    }

    /// Feed a pointer sample. `touch(mesh, chunk)` is called once per chunk
    /// before it is first modified by this call.
    pub fn add_sample(&mut self, mesh: &mut Mesh, sample: StrokeSample, touch: &mut dyn FnMut(&Mesh, u32)) {
        if self.settings.kind == BrushKind::Grab {
            self.grab_sample(mesh, sample, touch);
            return;
        }
        let Some(last) = self.last else {
            self.last = Some(sample);
            self.carry = 0.0;
            self.apply_dab(mesh, sample, touch);
            return;
        };
        let step = (self.settings.radius * self.settings.spacing).max(1e-6);
        let seg = sample.position - last.position;
        let len = seg.length();
        let mut d = step - self.carry;
        while d <= len {
            let t = d / len;
            let s = StrokeSample {
                position: last.position + seg * t,
                normal: last.normal.lerp(sample.normal, t).normalize_or(sample.normal),
                pressure: last.pressure + (sample.pressure - last.pressure) * t,
            };
            self.apply_dab(mesh, s, touch);
            d += step;
        }
        self.carry = len - (d - step);
        self.last = Some(sample);
    }

    fn make_dab(&self, s: StrokeSample) -> Dab {
        let p = s.pressure.clamp(0.0, 1.0);
        let radius =
            if self.settings.pressure_radius { self.settings.radius * (0.2 + 0.8 * p) } else { self.settings.radius };
        let strength =
            if self.settings.pressure_strength { self.settings.strength * p } else { self.settings.strength };
        Dab { center: s.position, normal: s.normal, radius, strength }
    }

    fn apply_dab(&mut self, mesh: &mut Mesh, s: StrokeSample, touch: &mut dyn FnMut(&Mesh, u32)) {
        let base = self.make_dab(s);
        self.dab_count += 1;
        for &m in &self.mirrors.clone() {
            let dab = Dab { center: base.center * m, normal: base.normal * m, ..base };
            apply_kernel(mesh, &self.settings, self.settings.kind, self.invert, &dab, touch);
            if self.settings.auto_smooth > 0.0
                && self.settings.kind != BrushKind::Smooth
                && self.settings.kind != BrushKind::Mask
            {
                let smooth = Dab { strength: self.settings.auto_smooth, ..dab };
                apply_kernel(mesh, &self.settings, BrushKind::Smooth, false, &smooth, touch);
            }
        }
    }

    fn grab_sample(&mut self, mesh: &mut Mesh, s: StrokeSample, touch: &mut dyn FnMut(&Mesh, u32)) {
        let Some(grab) = &self.grab else {
            // First sample: capture the vertices under each mirrored brush.
            let dab = self.make_dab(StrokeSample { pressure: 1.0, ..s });
            let captured = self
                .mirrors
                .iter()
                .map(|&m| {
                    let center = dab.center * m;
                    let normal = dab.normal * m;
                    let mut verts = Vec::new();
                    for c in mesh.chunks_in_sphere(center, dab.radius) {
                        for v in mesh.chunks[c as usize].vertices.clone() {
                            if let Some(w) = vertex_weight(mesh, &self.settings, &Dab { center, normal, ..dab }, v) {
                                verts.push((v, w, mesh.positions[v as usize]));
                            }
                        }
                    }
                    (m, verts)
                })
                .collect();
            self.grab = Some(GrabState { origin: s.position, captured });
            self.last = Some(s);
            return;
        };
        let delta = s.position - grab.origin;
        let mut moved = Vec::new();
        let mut chunks: Vec<u32> =
            grab.captured.iter().flat_map(|(_, vs)| vs.iter().map(|&(v, _, _)| mesh.vertex_chunk(v))).collect();
        chunks.sort_unstable();
        chunks.dedup();
        for c in chunks {
            touch(mesh, c);
        }
        for (m, verts) in &grab.captured {
            let d = delta * *m;
            for &(v, w, orig) in verts {
                mesh.positions[v as usize] = orig + d * w;
                moved.push(v);
            }
        }
        mesh.update_after_move(&moved);
        self.dab_count += 1;
        self.last = Some(s);
    }
}

/// Falloff weight × strength × (1 − mask) for a vertex, or None if outside.
fn vertex_weight(mesh: &Mesh, settings: &BrushSettings, dab: &Dab, v: u32) -> Option<f32> {
    let p = mesh.positions[v as usize];
    let d2 = p.distance_squared(dab.center);
    if d2 > dab.radius * dab.radius {
        return None;
    }
    if settings.front_faces_only && mesh.normals[v as usize].dot(dab.normal) < 0.0 {
        return None;
    }
    let f = settings.falloff.eval(d2.sqrt() / dab.radius);
    let mask = if settings.kind == BrushKind::Mask { 0.0 } else { mesh.mask[v as usize] };
    let w = f * (1.0 - mask);
    (w > 0.0).then_some(w)
}

fn apply_kernel(
    mesh: &mut Mesh,
    settings: &BrushSettings,
    kind: BrushKind,
    invert: bool,
    dab: &Dab,
    touch: &mut dyn FnMut(&Mesh, u32),
) {
    let chunks = mesh.chunks_in_sphere(dab.center, dab.radius);
    if chunks.is_empty() {
        return;
    }
    let settings = BrushSettings { kind, ..*settings };
    let sign = if invert { -1.0 } else { 1.0 };

    // Gather affected vertices and weights in parallel per chunk.
    let affected: Vec<(u32, f32)> = {
        let m: &Mesh = mesh;
        chunks
            .par_iter()
            .flat_map_iter(|&c| {
                m.chunks[c as usize]
                    .vertices
                    .clone()
                    .filter_map(move |v| vertex_weight(m, &settings, dab, v).map(|w| (v, w)))
            })
            .collect()
    };
    if affected.is_empty() {
        return;
    }
    for &c in &chunks {
        touch(mesh, c);
    }

    if kind == BrushKind::Mask {
        for &(v, w) in &affected {
            let m = &mut mesh.mask[v as usize];
            *m = (*m + sign * w * dab.strength).clamp(0.0, 1.0);
        }
        for c in chunks {
            mesh.mark_dirty(c);
        }
        return;
    }

    // Region statistics used by plane-based brushes.
    let (wsum, area_normal, area_center) =
        affected.iter().fold((0.0, Vec3::ZERO, Vec3::ZERO), |(ws, n, c), &(v, w)| {
            (ws + w, n + mesh.normals[v as usize] * w, c + mesh.positions[v as usize] * w)
        });
    let area_normal = area_normal.try_normalize().unwrap_or(dab.normal);
    let area_center = area_center / wsum.max(1e-12);
    // Displacement scale: a full-strength dab moves the surface ~10% of the radius.
    let amount = dab.radius * 0.1 * dab.strength;

    let new_positions: Vec<(u32, Vec3)> = {
        let m: &Mesh = mesh;
        affected
            .par_iter()
            .map(|&(v, w)| {
                let p = m.positions[v as usize];
                let np = match kind {
                    BrushKind::Draw => p + area_normal * (sign * amount * w),
                    BrushKind::Inflate => p + m.normals[v as usize] * (sign * amount * w),
                    BrushKind::Smooth => {
                        let n = m.neighbors(v);
                        if n.is_empty() {
                            p
                        } else {
                            let avg = n.iter().map(|&i| m.positions[i as usize]).sum::<Vec3>() / n.len() as f32;
                            p.lerp(avg, (w * dab.strength).min(1.0))
                        }
                    }
                    BrushKind::Flatten | BrushKind::Clay => {
                        // Clay builds toward a plane slightly above the surface;
                        // Flatten pulls everything onto the mean plane.
                        let (plane, one_sided) = if kind == BrushKind::Clay {
                            (area_center + area_normal * (sign * dab.radius * 0.15 * dab.strength), true)
                        } else {
                            (area_center, false)
                        };
                        let d = (plane - p).dot(area_normal);
                        if one_sided && d * sign < 0.0 {
                            p
                        } else {
                            p + area_normal * (d * (w * dab.strength).min(1.0))
                        }
                    }
                    BrushKind::Grab | BrushKind::Mask => p,
                };
                (v, np)
            })
            .collect()
    };

    let mut moved = Vec::with_capacity(new_positions.len());
    for (v, p) in new_positions {
        mesh.positions[v as usize] = p;
        moved.push(v);
    }
    mesh.update_after_move(&moved);
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    fn top_sample() -> StrokeSample {
        StrokeSample { position: Vec3::Y, normal: Vec3::Y, pressure: 1.0 }
    }

    fn run(kind: BrushKind, invert: bool, sym: Symmetry) -> (Mesh, Mesh, Vec<u32>) {
        let mut mesh = primitives::icosphere(4, 1.0);
        let before = mesh.clone();
        let settings = BrushSettings { kind, radius: 0.3, strength: 1.0, ..Default::default() };
        let mut stroke = Stroke::new(settings, sym, invert);
        let mut touched = Vec::new();
        stroke.add_sample(&mut mesh, top_sample(), &mut |_, c| touched.push(c));
        (before, mesh, touched)
    }

    #[test]
    fn draw_raises_and_invert_lowers() {
        let (b, a, touched) = run(BrushKind::Draw, false, Symmetry::default());
        assert!(a.bounds().max.y > b.bounds().max.y + 0.01);
        assert!(!touched.is_empty());
        let (b, a, _) = run(BrushKind::Draw, true, Symmetry::default());
        assert!(a.bounds().max.y < b.bounds().max.y - 0.01);
    }

    #[test]
    fn far_vertices_untouched() {
        let (b, a, _) = run(BrushKind::Inflate, false, Symmetry::default());
        for (p0, p1) in b.positions.iter().zip(&a.positions) {
            if p0.distance(Vec3::Y) > 0.31 {
                assert_eq!(p0, p1);
            }
        }
    }

    #[test]
    fn mask_protects() {
        let mut mesh = primitives::icosphere(4, 1.0);
        mesh.mask.iter_mut().for_each(|m| *m = 1.0);
        let before = mesh.positions.clone();
        let mut s = Stroke::new(BrushSettings::default(), Symmetry::default(), false);
        s.add_sample(&mut mesh, top_sample(), &mut |_, _| {});
        assert_eq!(before, mesh.positions);
    }

    #[test]
    fn mask_brush_paints_mask() {
        let (_, a, _) = run(BrushKind::Mask, false, Symmetry::default());
        assert!(a.mask.iter().any(|&m| m > 0.5));
        assert!(a.mask.iter().all(|&m| (0.0..=1.0).contains(&m)));
    }

    #[test]
    fn symmetry_mirrors_effect() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let settings = BrushSettings { radius: 0.3, strength: 1.0, ..Default::default() };
        let mut s = Stroke::new(settings, Symmetry { x: true, ..Default::default() }, false);
        s.add_sample(&mut mesh, StrokeSample { position: Vec3::X, normal: Vec3::X, pressure: 1.0 }, &mut |_, _| {});
        let b = mesh.bounds();
        assert!((b.max.x + b.min.x).abs() < 1e-3, "{b:?}");
        assert!(b.max.x > 1.01);
    }

    #[test]
    fn smooth_reduces_spike() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let top = (0..mesh.vertex_count() as u32)
            .max_by(|&a, &b| mesh.positions[a as usize].y.total_cmp(&mesh.positions[b as usize].y))
            .unwrap();
        mesh.positions[top as usize] *= 1.5;
        mesh.update_after_move(&[top]);
        let settings = BrushSettings {
            kind: BrushKind::Smooth,
            radius: 0.3,
            strength: 1.0,
            front_faces_only: false,
            ..Default::default()
        };
        let mut s = Stroke::new(settings, Symmetry::default(), false);
        let position = mesh.positions[top as usize];
        s.add_sample(&mut mesh, StrokeSample { position, normal: Vec3::Y, pressure: 1.0 }, &mut |_, _| {});
        assert!(mesh.positions[top as usize].length() < 1.4);
    }

    #[test]
    fn grab_moves_captured_region() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let settings = BrushSettings { kind: BrushKind::Grab, radius: 0.3, strength: 1.0, ..Default::default() };
        let mut s = Stroke::new(settings, Symmetry::default(), false);
        s.add_sample(&mut mesh, top_sample(), &mut |_, _| {});
        s.add_sample(&mut mesh, StrokeSample { position: Vec3::Y * 1.5, ..top_sample() }, &mut |_, _| {});
        assert!(mesh.bounds().max.y > 1.4);
    }

    #[test]
    fn spacing_emits_multiple_dabs() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let settings = BrushSettings { radius: 0.2, spacing: 0.25, ..Default::default() };
        let mut s = Stroke::new(settings, Symmetry::default(), false);
        let n = Vec3::Z;
        s.add_sample(
            &mut mesh,
            StrokeSample { position: Vec3::new(-0.5, 0.0, 0.9), normal: n, pressure: 1.0 },
            &mut |_, _| {},
        );
        s.add_sample(
            &mut mesh,
            StrokeSample { position: Vec3::new(0.5, 0.0, 0.9), normal: n, pressure: 1.0 },
            &mut |_, _| {},
        );
        // 1.0 / (0.2 * 0.25) = 20 dabs + the first (the last may round away).
        assert!((20..=21).contains(&s.dab_count()), "{}", s.dab_count());
    }
}
