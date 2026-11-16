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
#[derive(Debug, Clone)]
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

/// Looks an alpha up by name, size and [`alpha_fingerprint`].
pub type AlphaFinder<'a> = dyn Fn(&str, u32, u64) -> Option<Arc<Alpha>> + 'a;

/// Bytes a saved recipe starts with, and the layout version after them.
const MAGIC: &[u8; 4] = b"OXSR";
const VERSION: u8 = 1;
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
    /// "OXSR"  version:u8 (1)
    /// brush:text  falloff:text  shape:text          keys as in brush presets
    /// radius strength focal_shift spacing auto_smooth gravity spray
    ///   noise_scale crease_pinch : f32 each
    /// flags:u8      1 pressure_strength, 2 pressure_radius, 4 front_faces_only,
    ///               8 invert, 16 grab_through, 32 has alpha
    /// symmetry:u8   1 x, 2 y, 4 z;  radial:u32  radial_axis:u8 (0 x, 1 y, 2 z)
    /// center:f32*3  view:f32*3  paint:f32*3
    /// [alpha: name:text size:u32 fingerprint:u64 rotation:f32 follow:u8 up:f32*3]
    /// events:u32 then per event  tag:u8
    ///   0 sample:   position:f32*3 normal:f32*3 pressure:f32 spacing:f32 strength:f32 max_dabs:u32
    ///   1 stamp:    position:f32*3 normal:f32*3 pressure:f32 repeats:u32
    ///   2 modulate: size:f32 strength:f32
    /// ```
    /// `text` is `len:u32` then UTF-8. The alpha's image isn't stored: it is
    /// found again by name, size and [`alpha_fingerprint`].
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(96 + self.events.len() * 41);
        b.extend_from_slice(MAGIC);
        b.push(VERSION);
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
        if let Some(a) = &self.alpha {
            put_text(&mut b, &a.alpha.name);
            b.extend_from_slice(&a.alpha.size.to_le_bytes());
            b.extend_from_slice(&alpha_fingerprint(&a.alpha).to_le_bytes());
            put_f32(&mut b, a.rotation);
            b.push(u8::from(a.follow_stroke));
            put_vec3(&mut b, a.up);
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
    /// looks an alpha up by name, size and fingerprint.
    ///
    /// RETURNS: an error for damaged bytes, a newer layout, a brush this
    /// version doesn't know, or an alpha that can't be found (the stroke
    /// then simply has no recipe).
    pub fn from_bytes(bytes: &[u8], find_alpha: &AlphaFinder) -> Result<StrokeRecipe, String> {
        let mut r = Reader { data: bytes, pos: 0 };
        if r.take(4)? != MAGIC {
            return Err("not a stroke recipe".into());
        }
        let version = r.u8()?;
        if version != VERSION {
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
        };
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
        let alpha = if flags & 32 != 0 {
            let name = r.text()?;
            let size = r.u32()?;
            let fingerprint = r.u64()?;
            let rotation = r.f32()?;
            let follow_stroke = r.u8()? != 0;
            let up = r.vec3()?;
            let alpha = find_alpha(&name, size, fingerprint).ok_or_else(|| format!("alpha {name:?} not found"))?;
            Some(AlphaStamp { alpha, rotation, follow_stroke, up })
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
}

/// A quick fingerprint of an alpha's image, to tell apart alphas with the
/// same name and size: FNV-1a over up to 4096 evenly spaced values (hashing
/// every value of a 4096 px alpha for every stroke would be slow).
pub fn alpha_fingerprint(alpha: &Alpha) -> u64 {
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
                ..Default::default()
            };
            let symmetry = Symmetry { x: true, ..Default::default() };
            let alpha = AlphaStamp {
                alpha: Arc::new(BuiltinAlpha::Scales.generate(64)),
                rotation: 0.3,
                follow_stroke: true,
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
            let find = |name: &str, size: u32, print: u64| {
                let a = BuiltinAlpha::Scales.generate(64);
                (a.name == name && a.size == size && alpha_fingerprint(&a) == print).then(|| Arc::new(a))
            };
            let loaded = StrokeRecipe::from_bytes(&bytes, &find).expect("reads back");
            assert_eq!(loaded.events, recipe.events);
            assert_eq!(loaded.settings, recipe.settings);
            let mut third = base.clone();
            loaded.replay(&mut third, &mut |_, _| {});
            assert_eq!(third.positions, live.positions, "{kind:?} replays exactly from bytes");
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
        let none = |_: &str, _: u32, _: u64| None;
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
                up: Vec3::Y,
            }),
            ..recipe
        };
        assert!(StrokeRecipe::from_bytes(&with_alpha.to_bytes(), &none).is_err());
    }
}
