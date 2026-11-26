//! Brush presets: a brush's settings saved under a name, to reuse and share.
//!
//! A preset is plain data (never code), stored as small `key = value` text
//! (`.oxbrush` files) or as a one-line **brush code** for pasting into a
//! chat or forum post:
//!
//! ```text
//! oxyb1:<base64url of deflate-compressed preset text>
//! ```
//!
//! Unknown keys and invalid values are ignored, so presets from newer
//! versions still load (missing settings keep their defaults). Alphas are
//! referenced by name: built-in alphas always resolve; a user-loaded alpha
//! resolves only if an image of that name is loaded, otherwise the preset
//! applies without an alpha.

use crate::{BrushKind, Falloff, PaintJitter};

/// Prefix of a brush code (bump the digit if the encoding ever changes).
pub const CODE_PREFIX: &str = "oxyb1:";
/// First line of a preset file.
pub const FILE_HEADER: &str = "# OxyBrush brush preset";
/// File extension for presets.
pub const EXTENSION: &str = "oxbrush";

#[derive(Debug, Clone, PartialEq)]
pub struct BrushPreset {
    pub name: String,
    pub kind: BrushKind,
    /// Screen-space radius in points.
    pub radius_pt: f32,
    pub strength: f32,
    pub falloff: Falloff,
    /// Moves the falloff's middle, -1..=1 (see `BrushSettings::focal_shift`).
    pub focal_shift: f32,
    pub spacing: f32,
    pub pressure_strength: bool,
    pub pressure_radius: bool,
    /// Strength and size curve overrides. None uses the tablet default.
    pub pressure_curves: [Option<[f32; 4]>; 2],
    pub front_faces_only: bool,
    pub accumulate: bool,
    pub paint_jitter: PaintJitter,
    pub auto_smooth: f32,
    /// Gravity, 0..=1 (see `BrushSettings::gravity`).
    pub gravity: f32,
    pub spray: f32,
    pub noise_scale: f32,
    /// Crease brush: pinch intensity factor (0.0 = groove only, 0.3 = default, 1.0 = sharp pinch).
    pub crease_pinch: f32,
    /// "freehand", "line" or "stamp" (interpreted by the app).
    pub stroke_mode: String,
    /// Stabilizer distance in points (0 = off).
    pub stabilizer_pt: f32,
    /// Pressure curve exponent (1 = linear).
    pub pressure_gamma: f32,
    /// Alpha name (a built-in alpha label or a user image name); None = no alpha.
    pub alpha: Option<String>,
    pub alpha_rotation_deg: f32,
    pub alpha_follow_stroke: bool,
    pub alpha_invert: bool,
    pub alpha_tile: bool,
    pub alpha_tile_repeats: f32,
    pub alpha_roll: bool,
    pub alpha_roll_distance: f32,
    pub alpha_rotate_jitter_deg: f32,
    pub alpha_rotate_jitter_per_dab: bool,
    /// Shape used when kind is BrushKind::InsertPrimitive.
    pub insert_shape: crate::InsertShape,
}

impl BrushPreset {
    /// A preset of `kind` with its default settings.
    pub fn new(name: impl Into<String>, kind: BrushKind) -> Self {
        Self {
            name: name.into(),
            kind,
            radius_pt: 50.0,
            strength: kind.default_strength(),
            falloff: Falloff::Smooth,
            focal_shift: 0.0,
            spacing: 0.15,
            pressure_strength: true,
            pressure_radius: false,
            pressure_curves: [None; 2],
            front_faces_only: false,
            accumulate: kind.default_accumulate(),
            paint_jitter: PaintJitter::default(),
            auto_smooth: 0.0,
            gravity: 0.0,
            spray: 0.0,
            noise_scale: 3.0,
            crease_pinch: 0.3,
            stroke_mode: "freehand".into(),
            stabilizer_pt: 0.0,
            pressure_gamma: 1.0,
            alpha: None,
            alpha_rotation_deg: 0.0,
            alpha_follow_stroke: false,
            alpha_invert: false,
            alpha_tile: false,
            alpha_tile_repeats: 1.0,
            alpha_roll: false,
            alpha_roll_distance: 1.0,
            alpha_rotate_jitter_deg: 0.0,
            alpha_rotate_jitter_per_dab: false,
            insert_shape: crate::InsertShape::Sphere,
        }
    }

    /// Serialize as `key = value` lines.
    pub fn to_text(&self) -> String {
        let mut s = format!("{FILE_HEADER}\n");
        let mut kv = |k: &str, v: String| s.push_str(&format!("{k} = {v}\n"));
        // The name is the only free text; keep it on one line.
        kv("name", one_line(&self.name));
        kv("brush", kind_key(self.kind).into());
        kv("radius_pt", self.radius_pt.to_string());
        kv("strength", self.strength.to_string());
        kv("falloff", falloff_key(self.falloff).into());
        kv("focal_shift", self.focal_shift.to_string());
        kv("spacing", self.spacing.to_string());
        kv("pressure_strength", self.pressure_strength.to_string());
        kv("pressure_radius", self.pressure_radius.to_string());
        for (key, shape) in PRESSURE_CURVE_KEYS.into_iter().zip(self.pressure_curves) {
            if let Some([x1, y1, x2, y2]) = shape {
                kv(key, format!("{x1}, {y1}, {x2}, {y2}"));
            }
        }
        kv("front_faces_only", self.front_faces_only.to_string());
        if self.kind == BrushKind::Paint && self.paint_jitter != PaintJitter::default() {
            kv("jitter_hue", self.paint_jitter.hue.to_string());
            kv("jitter_saturation", self.paint_jitter.saturation.to_string());
            kv("jitter_value", self.paint_jitter.value.to_string());
            kv("jitter_per_dab", self.paint_jitter.per_dab.to_string());
        }
        if !self.accumulate {
            kv("accumulate", "false".into());
        }
        kv("auto_smooth", self.auto_smooth.to_string());
        kv("gravity", self.gravity.to_string());
        kv("spray", self.spray.to_string());
        kv("noise_scale", self.noise_scale.to_string());
        kv("crease_pinch", self.crease_pinch.to_string());
        kv("stroke_mode", self.stroke_mode.clone());
        kv("stabilizer_pt", self.stabilizer_pt.to_string());
        kv("pressure_gamma", self.pressure_gamma.to_string());
        kv("alpha", self.alpha.as_deref().map(one_line).unwrap_or_default());
        kv("alpha_rotation_deg", self.alpha_rotation_deg.to_string());
        kv("alpha_follow_stroke", self.alpha_follow_stroke.to_string());
        kv("alpha_invert", self.alpha_invert.to_string());
        if self.alpha_tile {
            kv("alpha_tile", "true".into());
            kv("alpha_tile_repeats", self.alpha_tile_repeats.to_string());
        }
        if self.alpha_roll {
            kv("alpha_roll", "true".into());
            kv("alpha_roll_distance", self.alpha_roll_distance.to_string());
        }
        if self.alpha_rotate_jitter_deg != 0.0 {
            kv("alpha_rotate_jitter_deg", self.alpha_rotate_jitter_deg.to_string());
            kv("alpha_rotate_jitter_per_dab", self.alpha_rotate_jitter_per_dab.to_string());
        }
        kv("insert_shape", shape_key(self.insert_shape).into());
        s
    }

    /// Parse preset text. Needs a known `brush`; everything else falls back
    /// to that brush's defaults when missing or out of range.
    pub fn from_text(text: &str) -> Result<BrushPreset, String> {
        let pairs: Vec<(&str, &str)> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim(), v.trim())))
            .collect();
        let get = |key: &str| pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
        let kind = get("brush").and_then(kind_from_key).ok_or("not a brush preset (unknown or missing brush)")?;
        let mut p = BrushPreset::new(get("name").filter(|n| !n.is_empty()).unwrap_or("Untitled brush"), kind);
        let num = |key: &str, range: std::ops::RangeInclusive<f32>| {
            get(key).and_then(|v| v.parse::<f32>().ok()).filter(|x| range.contains(x))
        };
        let flag = |key: &str| get(key).and_then(|v| v.parse::<bool>().ok());
        p.pressure_curves = PRESSURE_CURVE_KEYS.map(|key| get(key).and_then(parse_pressure_shape));
        if let Some(x) = num("radius_pt", 1.0..=1000.0) {
            p.radius_pt = x;
        }
        if let Some(x) = num("strength", 0.0..=1.0) {
            p.strength = x;
        }
        if let Some(f) = get("falloff").and_then(falloff_from_key) {
            p.falloff = f;
        }
        if let Some(x) = num("focal_shift", -1.0..=1.0) {
            p.focal_shift = x;
        }
        if let Some(x) = num("spacing", 0.01..=2.0) {
            p.spacing = x;
        }
        if let Some(b) = flag("pressure_strength") {
            p.pressure_strength = b;
        }
        if let Some(b) = flag("pressure_radius") {
            p.pressure_radius = b;
        }
        if let Some(b) = flag("front_faces_only") {
            p.front_faces_only = b;
        }
        // Older presets omitted the key when accumulation was on.
        p.accumulate = flag("accumulate").unwrap_or(true);
        if kind == BrushKind::Paint {
            p.paint_jitter = PaintJitter {
                hue: num("jitter_hue", 0.0..=180.0).unwrap_or_default(),
                saturation: num("jitter_saturation", 0.0..=100.0).unwrap_or_default(),
                value: num("jitter_value", 0.0..=100.0).unwrap_or_default(),
                per_dab: flag("jitter_per_dab").unwrap_or_default(),
            };
        }
        if let Some(x) = num("auto_smooth", 0.0..=1.0) {
            p.auto_smooth = x;
        }
        if let Some(x) = num("gravity", 0.0..=1.0) {
            p.gravity = x;
        }
        if let Some(x) = num("spray", 0.0..=1.0) {
            p.spray = x;
        }
        if let Some(x) = num("noise_scale", 0.1..=100.0) {
            p.noise_scale = x;
        }
        if let Some(x) = num("crease_pinch", 0.0..=2.0) {
            p.crease_pinch = x;
        }
        if let Some(m) = get("stroke_mode").filter(|m| ["freehand", "line", "stamp"].contains(m)) {
            p.stroke_mode = m.into();
        }
        if let Some(x) = num("stabilizer_pt", 0.0..=500.0) {
            p.stabilizer_pt = x;
        }
        if let Some(x) = num("pressure_gamma", 0.1..=10.0) {
            p.pressure_gamma = x;
        }
        p.alpha = get("alpha").filter(|a| !a.is_empty()).map(str::to_owned);
        if let Some(x) = num("alpha_rotation_deg", -360.0..=360.0) {
            p.alpha_rotation_deg = x;
        }
        if let Some(b) = flag("alpha_follow_stroke") {
            p.alpha_follow_stroke = b;
        }
        if let Some(b) = flag("alpha_invert") {
            p.alpha_invert = b;
        }
        if let Some(b) = flag("alpha_tile") {
            p.alpha_tile = b;
        }
        if let Some(x) = num("alpha_tile_repeats", 0.05..=32.0) {
            p.alpha_tile_repeats = x;
        }
        if let Some(b) = flag("alpha_roll") {
            p.alpha_roll = b;
        }
        if let Some(x) = num("alpha_roll_distance", 0.05..=32.0) {
            p.alpha_roll_distance = x;
        }
        if let Some(x) = num("alpha_rotate_jitter_deg", 0.0..=180.0) {
            p.alpha_rotate_jitter_deg = x;
        }
        if let Some(b) = flag("alpha_rotate_jitter_per_dab") {
            p.alpha_rotate_jitter_per_dab = b;
        }
        if let Some(s) = get("insert_shape").and_then(shape_from_key) {
            p.insert_shape = s;
        }
        Ok(p)
    }

    /// One-line brush code for sharing.
    pub fn to_code(&self) -> String {
        let packed = miniz_oxide::deflate::compress_to_vec(self.to_text().as_bytes(), 9);
        format!("{CODE_PREFIX}{}", base64url_encode(&packed))
    }

    /// Read a brush code (surrounding whitespace and quotes are ignored).
    pub fn from_code(code: &str) -> Result<BrushPreset, String> {
        let code = code.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`');
        let body = code.strip_prefix(CODE_PREFIX).ok_or("not an OxyBrush brush code (they start with oxyb1:)")?;
        let packed = base64url_decode(body).ok_or("the brush code is damaged (invalid characters)")?;
        // Presets are tiny; cap the inflated size so a crafted code can't
        // use much memory.
        let text = miniz_oxide::inflate::decompress_to_vec_with_limit(&packed, 64 * 1024)
            .map_err(|_| "the brush code is damaged (could not unpack it)")?;
        BrushPreset::from_text(&String::from_utf8_lossy(&text))
    }
}

const PRESSURE_CURVE_KEYS: [&str; 2] = ["pressure_shape", "pressure_shape_size"];

/// Read the four unit-square control coordinates of a pressure curve.
pub fn parse_pressure_shape(text: &str) -> Option<[f32; 4]> {
    let mut values = text.split(',');
    let mut shape = [0.0; 4];
    for point in &mut shape {
        *point = values.next()?.trim().parse().ok().filter(|x| (0.0..=1.0).contains(x))?;
    }
    values.next().is_none().then_some(shape)
}

fn one_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ").trim().to_owned()
}

/// Stable file keys (independent of display labels).
pub fn kind_key(k: BrushKind) -> &'static str {
    match k {
        BrushKind::Draw => "draw",
        BrushKind::Clay => "clay",
        BrushKind::ClayStrips => "clay_strips",
        BrushKind::Smooth => "smooth",
        BrushKind::Inflate => "inflate",
        BrushKind::Flatten => "flatten",
        BrushKind::Amplify => "amplify",
        BrushKind::SceneProject => "scene_project",
        BrushKind::Pinch => "pinch",
        BrushKind::Crease => "crease",
        BrushKind::SharpCrease => "sharp_crease",
        BrushKind::Layer => "layer",
        BrushKind::Grab => "grab",
        BrushKind::SnakeHook => "snake_hook",
        BrushKind::TopoMove => "topological_move",
        BrushKind::Polish => "polish",
        BrushKind::PlanarTrim => "planar_trim",
        BrushKind::Noise => "noise",
        BrushKind::KelvinGrab => "kelvin_grab",
        BrushKind::KelvinTwist => "kelvin_twist",
        BrushKind::KelvinScale => "kelvin_scale",
        BrushKind::CurveMove => "curve_move",
        BrushKind::Mask => "mask",
        BrushKind::Paint => "paint",
        BrushKind::PaintGroups => "paint_groups",
        BrushKind::InsertPrimitive => "insert_primitive",
        BrushKind::Tube => "tube",
        BrushKind::PlaneClip => "plane_clip",
        BrushKind::LineSplit => "line_split",
        BrushKind::CutAway => "cut_away",
    }
}

pub fn kind_from_key(key: &str) -> Option<BrushKind> {
    let key = legacy_kind_key(key);
    BrushKind::ALL.into_iter().find(|k| kind_key(*k) == key)
}

/// Keys used by older versions for brushes that have since been renamed.
pub fn legacy_kind_key(key: &str) -> &str {
    match key {
        "clip_curve" => "plane_clip",
        "slice_curve" => "line_split",
        "trim_curve" => "cut_away",
        other => other,
    }
}

pub fn shape_key(s: crate::InsertShape) -> &'static str {
    match s {
        crate::InsertShape::Sphere => "sphere",
        crate::InsertShape::QuadSphere => "quad_sphere",
        crate::InsertShape::UvSphere => "uv_sphere",
        crate::InsertShape::Cube => "cube",
        crate::InsertShape::Cylinder => "cylinder",
        crate::InsertShape::Cone => "cone",
        crate::InsertShape::Torus => "torus",
        crate::InsertShape::Plane => "plane",
    }
}

pub fn shape_from_key(key: &str) -> Option<crate::InsertShape> {
    crate::InsertShape::ALL.into_iter().find(|s| shape_key(*s) == key)
}

pub(crate) fn falloff_key(f: Falloff) -> &'static str {
    match f {
        Falloff::Smooth => "smooth",
        Falloff::Sharp => "sharp",
        Falloff::Linear => "linear",
        Falloff::Constant => "constant",
    }
}

pub(crate) fn falloff_from_key(key: &str) -> Option<Falloff> {
    Falloff::ALL.into_iter().find(|f| falloff_key(*f) == key)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64url without padding (RFC 4648 §5).
fn base64url_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..chunk.len() + 1 {
            out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let val = |c: u8| B64.iter().position(|&x| x == c).map(|p| p as u32);
    let bytes = s.as_bytes();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

/// A few original starter presets (shipped with the app).
pub fn starter_presets() -> Vec<BrushPreset> {
    let mut soft_clay = BrushPreset::new("Soft clay buildup", BrushKind::Clay);
    soft_clay.accumulate = true;
    soft_clay.strength = 0.25;
    soft_clay.auto_smooth = 0.3;
    let mut fine_crease = BrushPreset::new("Fine wrinkles", BrushKind::Crease);
    fine_crease.radius_pt = 14.0;
    fine_crease.strength = 0.5;
    fine_crease.falloff = Falloff::Sharp;
    fine_crease.stabilizer_pt = 20.0;
    fine_crease.crease_pinch = 0.5;
    let mut scales = BrushPreset::new("Scale stamps", BrushKind::Draw);
    scales.stroke_mode = "stamp".into();
    scales.falloff = Falloff::Constant;
    scales.alpha = Some("Scales".into());
    scales.strength = 0.5;
    let mut pores = BrushPreset::new("Skin pores", BrushKind::Noise);
    pores.radius_pt = 60.0;
    pores.strength = 0.12;
    pores.noise_scale = 9.0;
    let mut smooth_line = BrushPreset::new("Smooth long strokes", BrushKind::Smooth);
    smooth_line.stabilizer_pt = 60.0;
    smooth_line.strength = 0.6;
    vec![soft_clay, fine_crease, scales, pores, smooth_line]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BrushPreset {
        let mut p = BrushPreset::new("Dragon scales = big", BrushKind::ClayStrips);
        p.radius_pt = 33.5;
        p.pressure_curves = [Some([0.4, 0.1, 0.6, 0.9]), Some([0.2, 0.3, 0.7, 0.8])];
        p.strength = 0.73;
        p.falloff = Falloff::Constant;
        p.focal_shift = -0.35;
        p.spray = 0.2;
        p.stroke_mode = "stamp".into();
        p.alpha = Some("Scales".into());
        p.alpha_rotation_deg = -45.0;
        p.alpha_follow_stroke = true;
        p
    }

    #[test]
    fn new_presets_use_brush_defaults_and_old_presets_keep_buildup() {
        for kind in [BrushKind::Draw, BrushKind::Clay, BrushKind::Inflate] {
            let preset = BrushPreset::new("New", kind);
            assert!(!preset.accumulate);
            assert!(!BrushPreset::from_text(&preset.to_text()).unwrap().accumulate);
            let old = format!("brush = {}", kind_key(kind));
            assert!(BrushPreset::from_text(&old).unwrap().accumulate);
        }
        let buildup = &starter_presets()[0];
        assert!(BrushPreset::from_text(&buildup.to_text()).unwrap().accumulate);
    }

    #[test]
    fn text_round_trip_and_tolerant_parsing() {
        assert!(!BrushPreset::new("Default", BrushKind::Draw).front_faces_only);
        let p = sample();
        assert_eq!(BrushPreset::from_text(&p.to_text()).unwrap(), p);
        // A name with '=' survives (split at the first '=').
        assert!(p.to_text().contains("name = Dragon scales = big"));
        // Unknown keys and bad values are ignored; missing values take defaults.
        let t = "brush = pinch\nstrength = 7\nfuture_key = 1\nfalloff = wobbly\nstroke_mode = rm -rf\n";
        let q = BrushPreset::from_text(t).unwrap();
        assert_eq!(q, BrushPreset::new("Untitled brush", BrushKind::Pinch));
        assert!(BrushPreset::from_text("strength = 0.5").is_err(), "a preset needs a brush");
        assert!(BrushPreset::from_text("brush = laser").is_err());
        // Invalid, extra or nonfinite coordinates leave the global default in effect.
        for invalid in ["NaN, 0, 1, 1", "0, 0, 1, 1, junk", "0, -1, 1, 1", "0, 1"] {
            let t = format!("brush = clay\npressure_shape = {invalid}\n");
            assert_eq!(BrushPreset::from_text(&t).unwrap().pressure_curves, [None; 2]);
        }
        // Gravity: kept, and out-of-range values fall back to off.
        let mut g = BrushPreset::new("Drape", BrushKind::Clay);
        g.gravity = 0.4;
        assert_eq!(BrushPreset::from_text(&g.to_text()).unwrap().gravity, 0.4);
        assert_eq!(BrushPreset::from_text("brush = clay\ngravity = 3").unwrap().gravity, 0.0);
    }

    #[test]
    fn codes_round_trip_and_reject_junk() {
        let p = sample();
        let code = p.to_code();
        assert!(code.starts_with(CODE_PREFIX));
        assert!(!code.contains(char::is_whitespace), "one line");
        assert_eq!(BrushPreset::from_code(&code).unwrap(), p);
        assert_eq!(BrushPreset::from_code(&format!("  \"{code}\"\n")).unwrap(), p, "pasted with quotes");
        assert!(BrushPreset::from_code("hello").is_err());
        assert!(BrushPreset::from_code("oxyb1:!!!!").is_err());
        assert!(BrushPreset::from_code("oxyb1:AAAA").is_err());
        // A huge inflated payload is refused.
        let bomb = miniz_oxide::deflate::compress_to_vec(&vec![b'a'; 1 << 20], 9);
        assert!(BrushPreset::from_code(&format!("{CODE_PREFIX}{}", base64url_encode(&bomb))).is_err());
    }

    #[test]
    fn base64url_matches_rfc_vectors() {
        // RFC 4648 §10 test vectors (padding removed).
        for (plain, enc) in [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64url_encode(plain.as_bytes()), enc);
            assert_eq!(base64url_decode(enc).unwrap(), plain.as_bytes());
        }
        assert_eq!(base64url_encode(&[0xfb, 0xff]), "-_8");
        assert!(base64url_decode("Z").is_none());
    }

    #[test]
    fn every_brush_kind_has_a_stable_key_and_starters_are_valid() {
        for k in BrushKind::ALL {
            assert_eq!(kind_from_key(kind_key(k)), Some(k));
        }
        for p in starter_presets() {
            assert_eq!(BrushPreset::from_text(&p.to_text()).unwrap(), p, "{}", p.name);
        }
    }
}
