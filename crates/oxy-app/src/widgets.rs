//! Hand-drawn icons and small widgets that make the interface friendlier.
//!
//! Every icon is painted procedurally with egui's painter: original work
//! (no icon fonts or third-party assets), crisp at any UI scale, and
//! automatically consistent with the theme colors.
//!
//! Brush icons show a cross-section of the surface: the clay profile after
//! the brush, with a dashed line for the surface before it where that helps
//! explain the effect (Smooth, Flatten, Pinch…).

use egui::{Color32, Painter, Pos2, Rect, Response, Role, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};
use oxy_sculpt::BrushKind;

pub const ACCENT: Color32 = Color32::from_rgb(0xE0, 0x7A, 0x4F);
pub const MASK_BLUE: Color32 = Color32::from_rgb(110, 150, 255);
const CLAY_TOP: Color32 = Color32::from_rgb(0xD9, 0x9A, 0x78);
const CLAY_BOTTOM: Color32 = Color32::from_rgb(0x7A, 0x45, 0x30);
const CLAY_EDGE: Color32 = Color32::from_rgb(0xF2, 0xC2, 0xA4);
const GHOST: Color32 = Color32::from_gray(150);

/// One-line explanation of each brush, shown in tooltips.
pub fn brush_description(k: BrushKind) -> &'static str {
    match k {
        BrushKind::Draw => "Raises the surface into a soft bump. Alt carves in.",
        BrushKind::Clay => "Builds up flat-topped layers of clay. Alt removes clay.",
        BrushKind::ClayStrips => "Lays down square strips of clay, like a flat tool.",
        BrushKind::Smooth => "Evens out bumps and noise. Hold Shift with any brush.",
        BrushKind::Inflate => "Puffs the surface outward along its normals. Alt deflates.",
        BrushKind::Flatten => "Pushes the surface toward a flat plane.",
        BrushKind::Pinch => "Pulls the surface together to sharpen edges and ridges.",
        BrushKind::Crease => "Cuts a sharp groove, for wrinkles and folds. Alt raises a ridge.",
        BrushKind::Layer => "Raises an even plateau of fixed height, however often you go over it.",
        BrushKind::Grab => "Drag to move a whole region of the surface.",
        BrushKind::SnakeHook => "Pulls out long shapes like horns and tentacles.",
        BrushKind::TopoMove => "Moves only the connected surface under the brush; nearby separate parts stay put.",
        BrushKind::Polish => "Flattens small bumps without shrinking the form.",
        BrushKind::PlanarTrim => "Cuts the surface down to a plane. Alt fills up to it.",
        BrushKind::Noise => "Adds procedural surface noise. Adjust the noise scale below.",
        BrushKind::Mask => "Paints a mask that protects areas from other brushes. Alt unmasks.",
    }
}

/// Number-key shortcut for a brush, if it has one.
pub fn brush_key(k: BrushKind) -> Option<&'static str> {
    const KEYS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
    BrushKind::ALL.iter().position(|b| *b == k).and_then(|i| KEYS.get(i).copied())
}

fn bump(x: f32, w: f32) -> f32 {
    (-(x / w) * (x / w)).exp()
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Surface height after the brush at `x` in -1..1 (0 = untouched; ~±0.8 max).
pub fn brush_profile(k: BrushKind, x: f32) -> f32 {
    let a = x.abs();
    match k {
        BrushKind::Draw => 0.55 * bump(x, 0.42),
        BrushKind::Clay => 0.42 * (1.7 * bump(x, 0.55)).min(1.0),
        BrushKind::ClayStrips => 0.32 * smoothstep(0.62, 0.5, a) + 0.14 * smoothstep(0.32, 0.24, a),
        BrushKind::Smooth => 0.06 * (x * 9.0).sin() * bump(x, 0.7),
        BrushKind::Inflate => 0.62 * (1.0 - (x / 0.72).powi(2)).max(0.0).sqrt(),
        BrushKind::Flatten | BrushKind::PlanarTrim => (0.6 * bump(x, 0.5)).min(0.28),
        BrushKind::Pinch => 0.6 * (-a / 0.13).exp(),
        BrushKind::Crease => -0.45 * (1.0 - a / 0.3).max(0.0) + 0.1 * bump(a - 0.42, 0.16),
        BrushKind::Layer => 0.3 * smoothstep(0.62, 0.52, a),
        BrushKind::Grab => 0.55 * bump(x - 0.3, 0.38),
        BrushKind::SnakeHook => 0.85 * bump(x - 0.1, 0.13) + 0.1 * bump(x, 0.5),
        BrushKind::TopoMove => {
            0.28 * smoothstep(0.3, 0.2, (x + 0.5).abs()) + 0.5 * smoothstep(0.3, 0.2, (x - 0.45).abs())
        }
        BrushKind::Polish => 0.32 * bump(x, 0.6),
        BrushKind::Noise => {
            (0.1 * ((x * 13.0).sin() + 0.6 * (x * 29.0 + 1.0).sin() + 0.4 * (x * 47.0 + 2.0).sin()) + 0.08)
                * bump(x, 0.6)
        }
        BrushKind::Mask => 0.18 * bump(x, 0.8),
    }
}

/// Surface before the brush, drawn dashed where it explains the effect.
fn brush_ghost(k: BrushKind, x: f32) -> Option<f32> {
    match k {
        BrushKind::Smooth => Some(0.26 * (x * 9.0).sin() * bump(x, 0.7)),
        BrushKind::Flatten | BrushKind::PlanarTrim => Some(0.6 * bump(x, 0.5)),
        BrushKind::Pinch => Some(0.38 * bump(x, 0.42)),
        BrushKind::Grab => Some(0.55 * bump(x + 0.3, 0.38)),
        BrushKind::TopoMove => Some(0.28 * smoothstep(0.3, 0.2, (x - 0.45).abs())),
        BrushKind::Polish => Some(0.32 * bump(x, 0.6) + 0.07 * (x * 15.0).sin() * bump(x, 0.6)),
        _ => None,
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn arrow(p: &Painter, from: Pos2, to: Pos2, stroke: Stroke) {
    p.line_segment([from, to], stroke);
    let d = (to - from).normalized() * 4.0;
    let n = vec2(-d.y, d.x);
    p.line_segment([to, to - d + n * 0.8], stroke);
    p.line_segment([to, to - d - n * 0.8], stroke);
}

/// Paint a brush icon (surface cross-section) into `rect`.
pub fn paint_brush_icon(p: &Painter, rect: Rect, k: BrushKind) {
    p.rect_filled(rect, 5.0, Color32::from_rgb(0x26, 0x28, 0x2D));
    let r = rect.shrink(3.0);
    let base = r.top() + r.height() * 0.68;
    let amp = r.height() * 0.55;
    let to_x = |x: f32| r.left() + (x + 1.0) * 0.5 * r.width();
    const N: usize = 40;
    let xs: Vec<f32> = (0..=N).map(|i| i as f32 / N as f32 * 2.0 - 1.0).collect();

    // Clay body: a strip of quads from the surface down to the bottom.
    let mut mesh = egui::Mesh::default();
    for &x in &xs {
        let y = base - brush_profile(k, x) * amp;
        let masked = k == BrushKind::Mask && x.abs() < 0.45;
        let (top, bottom) =
            if masked { (MASK_BLUE, lerp_color(MASK_BLUE, CLAY_BOTTOM, 0.6)) } else { (CLAY_TOP, CLAY_BOTTOM) };
        mesh.colored_vertex(pos2(to_x(x), y), top);
        mesh.colored_vertex(pos2(to_x(x), r.bottom()), bottom);
    }
    for i in 0..N as u32 {
        let (a, b, c, d) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        mesh.add_triangle(a, b, c);
        mesh.add_triangle(b, d, c);
    }
    p.add(Shape::mesh(mesh));
    let surface: Vec<Pos2> = xs.iter().map(|&x| pos2(to_x(x), base - brush_profile(k, x) * amp)).collect();
    p.add(Shape::line(surface, Stroke::new(1.2, CLAY_EDGE)));

    if brush_ghost(k, 0.0).is_some() {
        let ghost: Vec<Pos2> =
            xs.iter().filter_map(|&x| brush_ghost(k, x).map(|y| pos2(to_x(x), base - y * amp))).collect();
        p.extend(Shape::dashed_line(&ghost, Stroke::new(1.0, GHOST), 2.5, 2.0));
    }

    let s = Stroke::new(1.4, Color32::WHITE);
    let at = |x: f32, h: f32| pos2(to_x(x), base - h * amp);
    match k {
        BrushKind::Grab => arrow(p, at(-0.55, 0.75), at(0.05, 0.75), s),
        BrushKind::TopoMove => arrow(p, at(0.45, 0.62), at(0.45, 0.9), s),
        BrushKind::SnakeHook => {
            let pts: Vec<Pos2> =
                (0..=8).map(|i| i as f32 / 8.0).map(|t| at(0.3 + 0.35 * t, 0.35 + 0.55 * t.sqrt())).collect();
            p.add(Shape::line(pts, s));
            arrow(p, at(0.6, 0.83), at(0.68, 0.95), s);
        }
        BrushKind::Inflate => {
            for (x0, x1, h0, h1) in [(-0.45, -0.75, 0.55, 0.8), (0.45, 0.75, 0.55, 0.8), (0.0, 0.0, 0.7, 1.0)] {
                arrow(p, at(x0, h0), at(x1, h1), Stroke::new(1.2, Color32::WHITE));
            }
        }
        BrushKind::PlanarTrim => {
            p.line_segment([at(-1.0, 0.28), at(1.0, 0.28)], Stroke::new(1.4, ACCENT));
        }
        _ => {}
    }
}

/// Generic interface icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Undo,
    Redo,
    Frame,
    Ortho,
    Eye,
    EyeOff,
    Mask,
    Groups,
    Plus,
    Up,
    Down,
    Help,
    Sphere,
    Cube,
    Plane,
    Open,
    Import,
    Orbit,
    Pan,
    Zoom,
    Solo,
}

/// Paint an icon into `rect` with color `c`.
pub fn paint_icon(p: &Painter, rect: Rect, icon: Icon, c: Color32) {
    let ctr = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let s = Stroke::new((r * 0.14).max(1.3), c);
    let arc = |from: f32, to: f32, rad: f32| -> Vec<Pos2> {
        (0..=16)
            .map(|i| {
                let t = from + (to - from) * i as f32 / 16.0;
                ctr + vec2(t.cos(), -t.sin()) * rad
            })
            .collect()
    };
    use std::f32::consts::PI;
    match icon {
        Icon::Undo | Icon::Redo => {
            // Arc over the top ending in a downward arrowhead: on the left for
            // undo (counter-clockwise, "go back"), mirrored for redo.
            let mirror = if icon == Icon::Undo { 1.0 } else { -1.0 };
            let pts: Vec<Pos2> = arc(-0.35 * PI, 1.0 * PI, r * 0.6)
                .into_iter()
                .map(|q| pos2(ctr.x + (q.x - ctr.x) * mirror, q.y + r * 0.12))
                .collect();
            let tip = pts[pts.len() - 1];
            p.add(Shape::line(pts, s));
            let h = r * 0.35;
            p.line_segment([tip, tip + vec2(-h, -h)], s);
            p.line_segment([tip, tip + vec2(h, -h)], s);
        }
        Icon::Frame => {
            let b = Rect::from_center_size(ctr, Vec2::splat(r * 1.5));
            let l = r * 0.45;
            for (corner, dx, dy) in [
                (b.left_top(), 1.0, 1.0),
                (b.right_top(), -1.0, 1.0),
                (b.left_bottom(), 1.0, -1.0),
                (b.right_bottom(), -1.0, -1.0),
            ] {
                p.line_segment([corner, corner + vec2(dx * l, 0.0)], s);
                p.line_segment([corner, corner + vec2(0.0, dy * l)], s);
            }
            p.circle_filled(ctr, r * 0.16, c);
        }
        Icon::Ortho | Icon::Cube => {
            // Isometric cube: three faces.
            let h = r * 0.78;
            let top = ctr + vec2(0.0, -h);
            let lt = ctr + vec2(-h * 0.87, -h * 0.5);
            let rt = ctr + vec2(h * 0.87, -h * 0.5);
            let lb = ctr + vec2(-h * 0.87, h * 0.5);
            let rb = ctr + vec2(h * 0.87, h * 0.5);
            let bot = ctr + vec2(0.0, h);
            if icon == Icon::Cube {
                let f = |k: f32| lerp_color(CLAY_BOTTOM, CLAY_TOP, k);
                p.add(Shape::convex_polygon(vec![top, rt, ctr, lt], f(1.0), Stroke::NONE));
                p.add(Shape::convex_polygon(vec![lt, ctr, bot, lb], f(0.65), Stroke::NONE));
                p.add(Shape::convex_polygon(vec![ctr, rt, rb, bot], f(0.3), Stroke::NONE));
            } else {
                p.add(Shape::closed_line(vec![top, rt, rb, bot, lb, lt], s));
                p.line_segment([lt, ctr], s);
                p.line_segment([rt, ctr], s);
                p.line_segment([ctr, bot], s);
            }
        }
        Icon::Eye | Icon::EyeOff => {
            let w = r * 0.9;
            let mut upper = arc(0.2 * PI, 0.8 * PI, w * 1.25);
            for q in &mut upper {
                q.y += w * 0.72;
            }
            let lower: Vec<Pos2> = upper.iter().map(|q| pos2(q.x, 2.0 * ctr.y - q.y)).collect();
            p.add(Shape::line(upper, s));
            p.add(Shape::line(lower, s));
            p.circle_filled(ctr, r * 0.28, c);
            if icon == Icon::EyeOff {
                p.line_segment([ctr + vec2(-r * 0.8, r * 0.8), ctr + vec2(r * 0.8, -r * 0.8)], s);
            }
        }
        Icon::Mask => {
            p.circle_stroke(ctr, r * 0.7, s);
            let half: Vec<Pos2> = arc(0.5 * PI, 1.5 * PI, r * 0.7);
            p.add(Shape::convex_polygon(half, MASK_BLUE, Stroke::NONE));
        }
        Icon::Groups => {
            let q = r * 0.62;
            let colors = [
                Color32::from_rgb(0xE0, 0x7A, 0x4F),
                Color32::from_rgb(0x6F, 0xB5, 0x8E),
                Color32::from_rgb(0x6E, 0x96, 0xE0),
                Color32::from_rgb(0xD6, 0xC0, 0x5A),
            ];
            for (i, col) in colors.iter().enumerate() {
                let o = vec2(if i % 2 == 0 { -q } else { 0.08 * r }, if i < 2 { -q } else { 0.08 * r });
                p.rect_filled(Rect::from_min_size(ctr + o, Vec2::splat(q * 0.92)), 1.5, *col);
            }
        }
        Icon::Plus => {
            let l = r * 0.6;
            p.line_segment([ctr - vec2(l, 0.0), ctr + vec2(l, 0.0)], s);
            p.line_segment([ctr - vec2(0.0, l), ctr + vec2(0.0, l)], s);
        }
        Icon::Up | Icon::Down => {
            let d = if icon == Icon::Up { -1.0 } else { 1.0 };
            let l = r * 0.5;
            p.add(Shape::line(
                vec![ctr + vec2(-l, -d * l * 0.5), ctr + vec2(0.0, d * l * 0.5), ctr + vec2(l, -d * l * 0.5)],
                s,
            ));
        }
        Icon::Help => {
            p.circle_stroke(ctr, r * 0.75, s);
            p.text(ctr, egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(r * 1.1), c);
        }
        Icon::Sphere => {
            // Shaded clay ball: concentric circles from dark rim to a lit spot.
            let rad = r * 0.85;
            for i in 0..12 {
                let t = i as f32 / 11.0;
                let off = vec2(-0.3, -0.3) * rad * t;
                p.circle_filled(ctr + off, rad * (1.0 - 0.75 * t), lerp_color(CLAY_BOTTOM, CLAY_TOP, t));
            }
        }
        Icon::Plane => {
            let w = r * 0.95;
            let pts = vec![
                ctr + vec2(-w * 0.5, -w * 0.35),
                ctr + vec2(w, -w * 0.35),
                ctr + vec2(w * 0.5, w * 0.35),
                ctr + vec2(-w, w * 0.35),
            ];
            p.add(Shape::convex_polygon(pts, lerp_color(CLAY_BOTTOM, CLAY_TOP, 0.7), Stroke::new(1.0, CLAY_EDGE)));
        }
        Icon::Open => {
            let b = Rect::from_center_size(ctr + vec2(0.0, r * 0.1), vec2(r * 1.6, r * 1.1));
            p.add(Shape::closed_line(
                vec![
                    b.left_top(),
                    b.left_top() + vec2(r * 0.5, 0.0),
                    b.left_top() + vec2(r * 0.65, r * 0.2),
                    b.right_top() + vec2(0.0, r * 0.2),
                    b.right_bottom(),
                    b.left_bottom(),
                ],
                s,
            ));
        }
        Icon::Orbit => {
            // Two curved arrows chasing each other around a dot.
            for (from, to) in [(0.15 * PI, 0.85 * PI), (1.15 * PI, 1.85 * PI)] {
                let pts = arc(from, to, r * 0.7);
                let tip = pts[pts.len() - 1];
                let prev = pts[pts.len() - 3];
                p.add(Shape::line(pts, s));
                let d = (tip - prev).normalized() * r * 0.35;
                let n = vec2(-d.y, d.x);
                p.line_segment([tip, tip - d + n * 0.8], s);
                p.line_segment([tip, tip - d - n * 0.8], s);
            }
            p.circle_filled(ctr, r * 0.14, c);
        }
        Icon::Pan => {
            let l = r * 0.8;
            for d in [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)] {
                arrow(p, ctr + d * r * 0.15, ctr + d * l, s);
            }
        }
        Icon::Zoom => {
            let lens = ctr + vec2(-r * 0.18, -r * 0.18);
            p.circle_stroke(lens, r * 0.5, s);
            p.line_segment(
                [lens + vec2(r * 0.36, r * 0.36), ctr + vec2(r * 0.8, r * 0.8)],
                Stroke::new(s.width * 1.6, c),
            );
        }
        Icon::Solo => {
            p.circle_filled(ctr, r * 0.38, c);
            for i in 0..3 {
                let a = PI * 0.5 + i as f32 * PI * 2.0 / 3.0;
                p.circle_stroke(
                    ctr + vec2(a.cos(), -a.sin()) * r * 0.78,
                    r * 0.16,
                    Stroke::new(1.0, c.gamma_multiply(0.5)),
                );
            }
        }
        Icon::Import => {
            let b = Rect::from_center_size(ctr + vec2(0.0, r * 0.45), vec2(r * 1.5, r * 0.6));
            p.add(Shape::line(vec![b.left_top(), b.left_bottom(), b.right_bottom(), b.right_top()], s));
            arrow(p, ctr + vec2(0.0, -r * 0.85), ctr + vec2(0.0, r * 0.35), s);
        }
    }
}

/// Square icon button with hover highlight, tooltip and screen-reader label.
pub fn icon_button(ui: &mut Ui, icon: Icon, selected: bool, enabled: bool, tooltip: &str) -> Response {
    let size = Vec2::splat(ui.spacing().interact_size.y.max(26.0));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| egui::WidgetInfo::selected(Role::Button, enabled, selected, tooltip));
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let bg = if selected {
            ACCENT.linear_multiply(0.45)
        } else if response.hovered() && enabled {
            v.widgets.hovered.weak_bg_fill
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 6.0, bg);
        let fg = if !enabled {
            v.weak_text_color().linear_multiply(0.5)
        } else if selected {
            Color32::WHITE
        } else {
            v.text_color()
        };
        paint_icon(ui.painter(), rect.shrink(5.0), icon, fg);
    }
    response.on_hover_text(tooltip)
}

/// A text toggle chip (e.g. the X/Y/Z symmetry axes).
pub fn chip(ui: &mut Ui, on: &mut bool, text: &str, tooltip: &str) -> Response {
    let mut r = ui.add(egui::Button::selectable(*on, egui::RichText::new(text).strong()).min_size(vec2(26.0, 26.0)));
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    r.on_hover_text(tooltip)
}

/// A key name drawn like a keyboard key.
pub fn keycap(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .stroke(Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color.gamma_multiply(1.6)))
        .fill(ui.visuals().extreme_bg_color)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).small().monospace());
        });
}

/// One row of the brush shelf: icon, name and shortcut key.
pub fn brush_row(ui: &mut Ui, k: BrushKind, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(Role::Button, true, selected, k.label()));
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let (bg, stroke) = if selected {
            (ACCENT.linear_multiply(0.28), Stroke::new(1.0, ACCENT))
        } else if response.hovered() {
            (v.widgets.hovered.weak_bg_fill, Stroke::NONE)
        } else {
            (Color32::TRANSPARENT, Stroke::NONE)
        };
        ui.painter().rect(rect, 6.0, bg, stroke, egui::StrokeKind::Inside);
        let icon = Rect::from_min_size(rect.left_top() + vec2(4.0, 3.0), Vec2::splat(30.0));
        paint_brush_icon(ui.painter(), icon, k);
        let text_color = if selected { Color32::WHITE } else { v.text_color() };
        ui.painter().text(
            pos2(icon.right() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            k.label(),
            egui::TextStyle::Button.resolve(ui.style()),
            text_color,
        );
        if let Some(key) = brush_key(k) {
            ui.painter().text(
                pos2(rect.right() - 8.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                key,
                egui::FontId::monospace(11.0),
                v.weak_text_color(),
            );
        }
    }
    let key = brush_key(k).map_or(String::new(), |k| format!("\nShortcut: {k}"));
    response.on_hover_text(format!("{}\n{}{key}", k.label(), brush_description(k)))
}

/// Large clickable card with an icon, a title and a one-line subtitle.
pub fn card(ui: &mut Ui, icon: Icon, title: &str, subtitle: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(124.0, 112.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(Role::Button, true, title));
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let (fill, stroke) = if response.hovered() {
            (v.widgets.hovered.weak_bg_fill, Stroke::new(1.0, ACCENT))
        } else {
            (v.faint_bg_color, Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color))
        };
        ui.painter().rect(rect, 10.0, fill, stroke, egui::StrokeKind::Inside);
        let icon_rect = Rect::from_center_size(rect.center_top() + vec2(0.0, 36.0), Vec2::splat(44.0));
        paint_icon(ui.painter(), icon_rect, icon, v.text_color());
        ui.painter().text(
            rect.center_top() + vec2(0.0, 74.0),
            egui::Align2::CENTER_CENTER,
            title,
            egui::FontId::proportional(14.5),
            v.strong_text_color(),
        );
        ui.painter().text(
            rect.center_top() + vec2(0.0, 93.0),
            egui::Align2::CENTER_CENTER,
            subtitle,
            egui::FontId::proportional(11.0),
            v.weak_text_color(),
        );
    }
    response
}

/// Button you drag on to navigate (orbit, pan, zoom), like dragging in the
/// viewport. Handy with a pen or touch screen. Read `drag_delta()`.
pub fn drag_button(ui: &mut Ui, icon: Icon, tooltip: &str) -> Response {
    let size = Vec2::splat(30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(Role::Button, true, tooltip));
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let bg = if response.dragged() {
            ACCENT.linear_multiply(0.45)
        } else if response.hovered() {
            v.widgets.hovered.weak_bg_fill
        } else {
            v.widgets.inactive.weak_bg_fill
        };
        ui.painter().rect_filled(rect, 6.0, bg);
        paint_icon(ui.painter(), rect.shrink(6.0), icon, v.text_color());
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    response.on_hover_text(tooltip)
}

/// A brush preview: a clay ball with the brush's effect pressed into it,
/// shaded with a fixed key light. Computed on the CPU (64 px, cached).
pub fn brush_preview_image(k: BrushKind, size: usize) -> egui::ColorImage {
    let asymmetric = matches!(k, BrushKind::Grab | BrushKind::SnakeHook | BrushKind::TopoMove);
    let height = |x: f32, y: f32| -> f32 {
        let (u, v) = ((x + 0.05) / 1.05, (y - 0.08) / 1.05);
        if asymmetric { brush_profile(k, u) * bump(v, 0.45) } else { brush_profile(k, (u * u + v * v).sqrt()) }
    };
    let key = egui::Vec2::new(-0.45, 0.55);
    let key_z = 0.7f32;
    let kl = (key.length_sq() + key_z * key_z).sqrt();
    let mut px = Vec::with_capacity(size * size);
    for j in 0..size {
        for i in 0..size {
            let x = (i as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let y = 1.0 - (j as f32 + 0.5) / size as f32 * 2.0;
            let (sx, sy) = (x / 0.92, y / 0.92);
            let rr = sx * sx + sy * sy;
            // Anti-aliased silhouette.
            let edge = ((1.0 - rr.sqrt()) * size as f32 * 0.46).clamp(0.0, 1.0);
            if edge <= 0.0 {
                px.push(Color32::TRANSPARENT);
                continue;
            }
            let nz = (1.0 - rr.min(1.0)).sqrt();
            let e = 2.0 / size as f32;
            let amp = 0.7;
            let gx = (height(sx + e, sy) - height(sx - e, sy)) / (2.0 * e) * amp;
            let gy = (height(sx, sy + e) - height(sx, sy - e)) / (2.0 * e) * amp;
            let n = egui::Vec2::new(sx - gx * nz, sy - gy * nz);
            let nzz = nz.max(0.05);
            let nl = (n.length_sq() + nzz * nzz).sqrt();
            let (nx, ny, nz2) = (n.x / nl, n.y / nl, nzz / nl);
            let diffuse = ((nx * key.x + ny * key.y + nz2 * key_z) / kl).max(0.0);
            let half = {
                let (hx, hy, hz) = (key.x / kl, key.y / kl, key_z / kl + 1.0);
                let hl = (hx * hx + hy * hy + hz * hz).sqrt();
                ((nx * hx + ny * hy + nz2 * hz) / hl).max(0.0)
            };
            let spec = half.powf(28.0) * 0.35;
            let rim = (1.0 - nz2).powi(3) * 0.2;
            let masked = k == BrushKind::Mask && height(sx, sy) > 0.12;
            // Planar Trim shares Flatten's profile; show its cut face as freshly trimmed (lighter).
            let trimmed = k == BrushKind::PlanarTrim && height(sx, sy) > 0.275;
            let base = if masked {
                [0.18, 0.26, 0.6]
            } else if trimmed {
                [0.8, 0.45, 0.3]
            } else {
                [0.62, 0.26, 0.15]
            };
            let lin = |b: f32| b * (0.1 + diffuse * 0.95) + spec + rim * 0.5;
            let srgb = |l: f32| {
                let l = l.clamp(0.0, 1.0);
                let s = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
                (s * 255.0 + 0.5) as u8
            };
            let a = (edge * 255.0) as u8;
            px.push(Color32::from_rgba_unmultiplied(srgb(lin(base[0])), srgb(lin(base[1])), srgb(lin(base[2])), a));
        }
    }
    egui::ColorImage::new([size, size], px)
}

/// The material's lit sphere as a round image.
pub fn material_image(m: oxy_render::MatcapPreset, size: usize) -> egui::ColorImage {
    let rgba = oxy_render::matcap::generate(m, size as u32);
    let px = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (x, y) = ((i % size) as f32 + 0.5, (i / size) as f32 + 0.5);
            let r = ((x / size as f32 * 2.0 - 1.0).powi(2) + (y / size as f32 * 2.0 - 1.0).powi(2)).sqrt();
            let a = ((0.96 - r) * size as f32 * 0.5).clamp(0.0, 1.0);
            Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (a * 255.0) as u8)
        })
        .collect();
    egui::ColorImage::new([size, size], px)
}

/// Texture cached in egui memory under `key`, created once with `make`.
pub fn cached_texture(
    ctx: &egui::Context,
    key: impl std::hash::Hash + std::fmt::Debug,
    make: impl FnOnce() -> egui::ColorImage,
) -> egui::TextureHandle {
    let id = egui::Id::unique(("oxy_tex", key));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let t = ctx.load_texture(format!("{id:?}"), make(), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

/// Brush preview texture.
pub fn brush_texture(ctx: &egui::Context, k: BrushKind) -> egui::TextureHandle {
    cached_texture(ctx, ("brush", k.label()), || brush_preview_image(k, 64))
}

/// Material preview texture.
pub fn material_texture(ctx: &egui::Context, m: oxy_render::MatcapPreset) -> egui::TextureHandle {
    cached_texture(ctx, ("material", m.label()), || material_image(m, 64))
}

/// A large preview tile (brush, alpha, material) with a caption; click to pick.
pub fn tile(ui: &mut Ui, tex: Option<&egui::TextureHandle>, caption: &str, tooltip: &str, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size + 16.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(Role::Button, true, tooltip));
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let img = Rect::from_min_size(rect.min, Vec2::splat(size));
        let stroke =
            if response.hovered() { Stroke::new(1.0, ACCENT) } else { Stroke::new(1.0, Color32::from_gray(52)) };
        ui.painter().rect(img, 6.0, Color32::from_rgb(0x2A, 0x2C, 0x31), stroke, egui::StrokeKind::Inside);
        match tex {
            Some(t) => {
                ui.painter().image(
                    t.id(),
                    img.shrink(4.0),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                ui.painter().text(
                    img.center(),
                    egui::Align2::CENTER_CENTER,
                    "None",
                    egui::FontId::proportional(11.0),
                    v.weak_text_color(),
                );
            }
        }
        // Small corner marker hints that the tile opens a picker.
        let c = img.right_bottom() - vec2(6.0, 6.0);
        ui.painter().add(Shape::convex_polygon(
            vec![c + vec2(0.0, -5.0), c, c + vec2(-5.0, 0.0)],
            v.weak_text_color(),
            Stroke::NONE,
        ));
        let text = egui::RichText::new(caption).small();
        let galley = egui::WidgetText::from(text).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            size,
            egui::TextStyle::Small,
        );
        ui.painter().galley(pos2(img.center().x - galley.size().x / 2.0, img.bottom() + 2.0), galley, v.text_color());
    }
    response.on_hover_text(tooltip)
}

/// Clickable thumbnail used inside the picker grids.
pub fn thumb(ui: &mut Ui, tex: Option<&egui::TextureHandle>, label: &str, selected: bool) -> Response {
    let size = 56.0;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size + 14.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(Role::Button, true, selected, label));
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let img = Rect::from_min_size(rect.min, Vec2::splat(size));
        let stroke = if selected {
            Stroke::new(1.5, ACCENT)
        } else if response.hovered() {
            Stroke::new(1.0, Color32::from_gray(140))
        } else {
            Stroke::NONE
        };
        ui.painter().rect(img, 5.0, Color32::from_rgb(0x2A, 0x2C, 0x31), stroke, egui::StrokeKind::Inside);
        if let Some(t) = tex {
            ui.painter().image(
                t.id(),
                img.shrink(3.0),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            ui.painter().text(
                img.center(),
                egui::Align2::CENTER_CENTER,
                "None",
                egui::FontId::proportional(11.0),
                v.weak_text_color(),
            );
        }
        let galley = egui::WidgetText::from(egui::RichText::new(label).size(10.0)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            size,
            egui::TextStyle::Small,
        );
        ui.painter().galley(pos2(img.center().x - galley.size().x / 2.0, img.bottom() + 1.0), galley, v.text_color());
    }
    response.on_hover_text(label)
}

/// Collapsible inspector section with a consistent header style.
pub fn section<R>(ui: &mut Ui, title: &str, default_open: bool, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let r = egui::CollapsingHeader::new(egui::RichText::new(title).strong().size(14.0))
        .default_open(default_open)
        .show(ui, add)
        .body_returned;
    ui.add_space(2.0);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_brush_has_help_and_a_distinct_icon() {
        let sample = |k: BrushKind| -> Vec<i32> {
            (0..=20).map(|i| (brush_profile(k, i as f32 / 10.0 - 1.0) * 1000.0) as i32).collect()
        };
        for (i, a) in BrushKind::ALL.iter().enumerate() {
            assert!(brush_description(*a).len() > 20, "{a:?}");
            for x in 0..=20 {
                let h = brush_profile(*a, x as f32 / 10.0 - 1.0);
                assert!(h.is_finite() && (-0.7..=0.95).contains(&h), "{a:?} profile out of range: {h}");
            }
            for b in &BrushKind::ALL[i + 1..] {
                // Flatten and Planar Trim share a profile; the plane line tells them apart.
                if matches!((a, b), (BrushKind::Flatten, BrushKind::PlanarTrim)) {
                    continue;
                }
                assert_ne!(sample(*a), sample(*b), "{a:?} and {b:?} icons look identical");
            }
        }
        assert_eq!(brush_key(BrushKind::Draw), Some("1"));
        assert_eq!(brush_key(BrushKind::Grab), Some("0"));
        assert_eq!(brush_key(BrushKind::Mask), None);
    }

    #[test]
    fn previews_are_round_and_distinct() {
        let images: Vec<_> = BrushKind::ALL.iter().map(|k| brush_preview_image(*k, 32)).collect();
        for (k, img) in BrushKind::ALL.iter().zip(&images) {
            assert_eq!(img.pixels.len(), 32 * 32);
            assert_eq!(img.pixels[0].a(), 0, "{k:?}: corners are transparent");
            assert_eq!(img.pixels[16 * 32 + 16].a(), 255, "{k:?}: center is opaque");
        }
        for i in 0..images.len() {
            for j in i + 1..images.len() {
                assert!(images[i].pixels != images[j].pixels, "{:?} vs {:?}", BrushKind::ALL[i], BrushKind::ALL[j]);
            }
        }
        let m = material_image(oxy_render::MatcapPreset::Jade, 32);
        assert_eq!(m.pixels[0].a(), 0);
        assert_eq!(m.pixels[16 * 32 + 16].a(), 255);
    }
}
