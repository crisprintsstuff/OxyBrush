//! Settings per brush (#305): each brush keeps its own falloff, focal
//! shift, spacing, stroke mode, stabilizer, spray, auto smooth, pressure
//! switches, alpha and the rest, as a [`BrushPreset`] (strength and
//! gravity were per brush already). Switching brush puts away the old
//! brush's settings and brings back the new one's; a brush not used yet
//! starts with the settings in effect. Size stays shared unless **Size per
//! brush** is on, since most sculptors keep one size across brushes. The
//! pen's calibration (Pen & tablet) is the same for every brush.
//!
//! Brushes can change in many places (shelf, keys, palette, quick menu),
//! so the swap happens in [`Editor::sync_brush_look`], every frame and
//! before a stroke, rather than at each of them.

use crate::editor::{Editor, StrokeMode};
use oxy_sculpt::{BrushKind, BrushPreset, PaintJitter};

impl Editor {
    /// Put away the previous brush's settings and bring back the current
    /// brush's, if the brush changed since the last call.
    pub fn sync_brush_look(&mut self) {
        let now = self.brush_kind;
        if self.look_kind == Some(now) {
            return;
        }
        if !self.settings_per_brush {
            self.look_kind = Some(now);
            return;
        }
        if let Some(old) = self.look_kind {
            self.put_away_look(old);
        }
        if let Some(look) = self.brush_looks.get(&now).cloned() {
            self.apply_look(&look);
        }
        self.look_kind = Some(now);
    }

    /// Keep the settings in effect as brush `kind`'s.
    pub(crate) fn put_away_look(&mut self, kind: oxy_sculpt::BrushKind) {
        if self.settings_per_brush {
            let mut look = self.capture_preset(kind.label());
            look.kind = kind;
            look.accumulate = self.brush_accumulates(kind);
            look.paint_jitter = self.paint_jitter(kind);
            look.pressure_curves = self.brush_pressure_overrides(kind);
            self.brush_looks.insert(kind, look);
        }
    }

    pub(crate) fn brush_accumulates(&self, kind: BrushKind) -> bool {
        self.brush_looks.get(&kind).map_or(kind.default_accumulate(), |look| look.accumulate)
    }

    pub(crate) fn set_brush_accumulate(&mut self, accumulate: bool) {
        let kind = self.brush_kind;
        let current = self.capture_preset(kind.label());
        self.brush_looks.entry(kind).or_insert(current).accumulate = accumulate;
    }

    pub(crate) fn paint_jitter(&self, kind: BrushKind) -> PaintJitter {
        if kind == BrushKind::Paint {
            self.brush_looks.get(&kind).map_or_else(PaintJitter::default, |look| look.paint_jitter)
        } else {
            PaintJitter::default()
        }
    }

    pub(crate) fn set_paint_jitter(&mut self, jitter: PaintJitter) {
        let current = self.capture_preset(BrushKind::Paint.label());
        self.brush_looks.entry(BrushKind::Paint).or_insert(current).paint_jitter = jitter;
    }

    pub(crate) fn brush_pressure_overrides(&self, kind: BrushKind) -> [Option<[f32; 4]>; 2] {
        self.brush_looks.get(&kind).map_or([None; 2], |look| look.pressure_curves)
    }

    pub(crate) fn set_brush_pressure_curve(&mut self, index: usize, shape: Option<[f32; 4]>) {
        let kind = self.brush_kind;
        if !self.brush_looks.contains_key(&kind) {
            self.brush_looks.insert(kind, self.capture_preset(kind.label()));
        }
        self.brush_looks.get_mut(&kind).expect("brush look exists").pressure_curves[index] = shape;
    }

    /// A brush's own settings, without changing the brush, the status line
    /// or the pen calibration; the size only with Size per brush.
    pub(crate) fn apply_look(&mut self, p: &BrushPreset) {
        let k = self.brush_kind;
        if self.size_per_brush {
            self.brush_radius_pt = p.radius_pt;
        }
        *self.strength_mut(k) = p.strength;
        *self.gravity_mut(k) = p.gravity;
        self.falloff = p.falloff;
        self.focal_shift = p.focal_shift;
        self.spacing = p.spacing;
        self.pressure_strength = p.pressure_strength;
        self.pressure_radius = p.pressure_radius;
        self.front_faces_only = p.front_faces_only;
        self.auto_smooth = p.auto_smooth;
        self.spray = p.spray;
        self.noise_scale = p.noise_scale;
        self.crease_pinch = p.crease_pinch;
        self.insert_shape = p.insert_shape;
        self.stroke_mode = StrokeMode::from_key(&p.stroke_mode).unwrap_or_default();
        self.stabilizer_pt = p.stabilizer_pt;
        self.alpha_rotation_deg = p.alpha_rotation_deg;
        self.alpha_follow_stroke = p.alpha_follow_stroke;
        self.alpha_invert = p.alpha_invert;
        // An alpha this project hasn't loaded leaves the brush without one.
        self.alpha =
            p.alpha.as_ref().and_then(|name| self.alphas.iter().position(|a| a.name.eq_ignore_ascii_case(name)));
    }

    /// The other brushes' settings, for saving (the current brush's are
    /// saved as the last brush), in brush order so the file doesn't churn.
    pub(crate) fn brush_looks_to_save(&self) -> Vec<BrushPreset> {
        let mut looks: Vec<BrushPreset> =
            self.brush_looks.values().filter(|l| l.kind != self.brush_kind).cloned().collect();
        let order = |k| oxy_sculpt::BrushKind::ALL.iter().position(|&x| x == k).unwrap_or(usize::MAX);
        looks.sort_by_key(|l| order(l.kind));
        looks
    }
}

#[cfg(test)]
mod tests {
    use crate::editor::{Editor, StrokeMode};
    use oxy_sculpt::{BrushKind, Falloff};

    #[test]
    fn accumulate_defaults_match_the_brush_and_buildup_preset() {
        let mut editor = Editor::default();
        for kind in [BrushKind::Draw, BrushKind::Clay, BrushKind::Inflate] {
            assert!(!editor.brush_accumulates(kind));
        }
        assert!(editor.brush_accumulates(BrushKind::ClayStrips));
        let buildup =
            oxy_sculpt::preset::starter_presets().into_iter().find(|p| p.name == "Soft clay buildup").unwrap();
        editor.apply_preset(&buildup);
        assert!(editor.brush_accumulates(BrushKind::Clay));
        assert!(!editor.brush_accumulates(BrushKind::Draw));
    }

    #[test]
    fn accumulate_is_per_brush_and_round_trips_when_settings_are_shared() {
        let mut editor = Editor::default();
        editor.remember_brushes = true;
        editor.settings_per_brush = false;
        editor.brush_kind = BrushKind::Draw;
        editor.set_brush_accumulate(true);
        editor.brush_kind = BrushKind::ClayStrips;
        editor.set_brush_accumulate(false);
        editor.brush_kind = BrushKind::Clay;
        editor.sync_brush_look();
        let settings = crate::settings::Settings::from_text(&editor.settings().to_text());
        let mut loaded = Editor::default();
        loaded.apply_settings(&settings);
        assert!(loaded.brush_accumulates(BrushKind::Draw));
        assert!(!loaded.brush_accumulates(BrushKind::ClayStrips));
        assert!(!loaded.brush_accumulates(BrushKind::Clay));
        loaded.apply_preset(&oxy_sculpt::BrushPreset::from_text("brush = clay").unwrap());
        assert!(loaded.brush_accumulates(BrushKind::Clay));
    }

    #[test]
    fn each_brush_keeps_its_own_settings_and_alpha() {
        let mut e = Editor::default();
        e.brush_kind = BrushKind::Clay;
        e.sync_brush_look();
        e.falloff = Falloff::Sharp;
        e.spacing = 0.05;
        e.stroke_mode = StrokeMode::Line;
        e.alpha = Some(1.min(e.alphas.len().saturating_sub(1)));
        let clay_alpha = e.alpha;
        e.brush_radius_pt = 30.0;

        // A brush used for the first time starts with what's in effect.
        e.brush_kind = BrushKind::Draw;
        e.sync_brush_look();
        assert_eq!(e.falloff, Falloff::Sharp);
        e.falloff = Falloff::Linear;
        e.spacing = 0.3;
        e.stroke_mode = StrokeMode::Freehand;
        e.alpha = None;
        e.brush_radius_pt = 80.0;

        e.brush_kind = BrushKind::Clay;
        e.sync_brush_look();
        assert_eq!((e.falloff, e.spacing, e.stroke_mode), (Falloff::Sharp, 0.05, StrokeMode::Line));
        assert_eq!(e.alpha, clay_alpha, "the alpha is the brush's too");
        assert_eq!(e.brush_radius_pt, 80.0, "size is shared unless Size per brush is on");

        e.size_per_brush = true;
        e.brush_kind = BrushKind::Draw;
        e.sync_brush_look();
        assert_eq!((e.falloff, e.spacing, e.alpha), (Falloff::Linear, 0.3, None));
        e.brush_radius_pt = 20.0;
        e.brush_kind = BrushKind::Clay;
        e.sync_brush_look();
        assert_eq!(e.brush_radius_pt, 80.0, "Clay's own size, put away when leaving it");

        // With Settings per brush off, nothing changes on a switch.
        e.settings_per_brush = false;
        e.falloff = Falloff::Smooth;
        e.brush_kind = BrushKind::Draw;
        e.sync_brush_look();
        assert_eq!(e.falloff, Falloff::Smooth);
    }

    #[test]
    fn a_preset_becomes_its_brush_settings_and_the_old_brush_keeps_its_own() {
        let mut e = Editor::default();
        e.brush_kind = BrushKind::Draw;
        e.sync_brush_look();
        e.falloff = Falloff::Sharp;
        let mut p = oxy_sculpt::BrushPreset::new("Soft clay", BrushKind::Clay);
        p.falloff = Falloff::Linear;
        e.apply_preset(&p);
        e.sync_brush_look();
        assert_eq!((e.brush_kind, e.falloff), (BrushKind::Clay, Falloff::Linear));
        e.brush_kind = BrushKind::Draw;
        e.sync_brush_look();
        assert_eq!(e.falloff, Falloff::Sharp, "Draw kept its own");
    }

    #[test]
    fn brush_settings_are_saved_and_read_back() {
        use crate::settings::Settings;
        let mut e = Editor::default();
        e.remember_brushes = true;
        e.brush_kind = BrushKind::Clay;
        e.sync_brush_look();
        e.falloff = Falloff::Sharp;
        e.brush_kind = BrushKind::Draw;
        e.sync_brush_look();
        e.falloff = Falloff::Linear;
        let text = e.settings().to_text();
        let mut back = Editor::default();
        back.apply_settings(&Settings::from_text(&text));
        assert_eq!((back.brush_kind, back.falloff), (BrushKind::Draw, Falloff::Linear));
        back.brush_kind = BrushKind::Clay;
        back.sync_brush_look();
        assert_eq!(back.falloff, Falloff::Sharp);
    }
}
