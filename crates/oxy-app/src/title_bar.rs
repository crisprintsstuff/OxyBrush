//! OxyBrush's own title bar (RFC 0028): on Windows and Linux the window is
//! made without the system's title bar, and the menu bar takes its place.
//! It holds the menus, the tabs and the minimize, maximize and close
//! buttons, moves the window when its empty part is dragged, and thin edges
//! around the window resize it.
//!
//! The UI only asks ([`WindowRequest`] in `Editor::window_request`); the
//! app carries the request out on the window, because egui has no window
//! of its own here.

use crate::editor::Editor;
use egui::{Color32, CursorIcon, Pos2, Rect, Sense, Stroke, Ui, vec2};

/// Something the title bar asks the window to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowRequest {
    /// Start moving the window with the pointer (the button is held).
    Drag,
    /// Start resizing from an edge or corner (the button is held).
    Resize(Edge),
    Minimize,
    /// Maximize, or restore when maximized.
    ToggleMaximize,
    /// Close the window, asking about unsaved tabs first, as the system
    /// close button does.
    Close,
    /// The system window menu at this point (in points), where there is one.
    Menu(Pos2),
}

/// A window edge or corner to resize from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl Edge {
    pub fn cursor(self) -> CursorIcon {
        match self {
            Edge::North => CursorIcon::ResizeNorth,
            Edge::South => CursorIcon::ResizeSouth,
            Edge::East => CursorIcon::ResizeEast,
            Edge::West => CursorIcon::ResizeWest,
            Edge::NorthEast => CursorIcon::ResizeNorthEast,
            Edge::NorthWest => CursorIcon::ResizeNorthWest,
            Edge::SouthEast => CursorIcon::ResizeSouthEast,
            Edge::SouthWest => CursorIcon::ResizeSouthWest,
        }
    }

    pub fn to_winit(self) -> winit::window::ResizeDirection {
        use winit::window::ResizeDirection as D;
        match self {
            Edge::North => D::North,
            Edge::South => D::South,
            Edge::East => D::East,
            Edge::West => D::West,
            Edge::NorthEast => D::NorthEast,
            Edge::NorthWest => D::NorthWest,
            Edge::SouthEast => D::SouthEast,
            Edge::SouthWest => D::SouthWest,
        }
    }
}

/// How far in from the window border a press resizes (points).
pub const EDGE: f32 = 5.0;
/// Near a corner (points along either side), a press resizes both ways.
const CORNER: f32 = 14.0;

/// The edge or corner of `screen` that `pos` is on, if any.
pub fn edge_at(screen: Rect, pos: Pos2) -> Option<Edge> {
    if !screen.contains(pos) {
        return None;
    }
    let near = |d: f32, band: f32| d < band;
    let (l, r) = (pos.x - screen.left(), screen.right() - pos.x);
    let (t, b) = (pos.y - screen.top(), screen.bottom() - pos.y);
    let on_side = near(l, EDGE) || near(r, EDGE) || near(t, EDGE) || near(b, EDGE);
    if !on_side {
        return None;
    }
    let (west, east) = (near(l, CORNER), near(r, CORNER));
    let (north, south) = (near(t, CORNER), near(b, CORNER));
    Some(match (north, south, west, east) {
        (true, _, true, _) => Edge::NorthWest,
        (true, _, _, true) => Edge::NorthEast,
        (_, true, true, _) => Edge::SouthWest,
        (_, true, _, true) => Edge::SouthEast,
        _ if near(t, EDGE) => Edge::North,
        _ if near(b, EDGE) => Edge::South,
        _ if near(l, EDGE) => Edge::West,
        _ => Edge::East,
    })
}

/// Empty bar kept free to move the window by, at the least (points).
pub const MIN_DRAG: f32 = 48.0;

/// Size of one window button for the layout.
fn button_size(ed: &Editor) -> egui::Vec2 {
    if ed.layout == crate::settings::UiLayout::Touch { vec2(52.0, 34.0) } else { vec2(44.0, 26.0) }
}

/// Width the three window buttons take (points).
pub fn buttons_width(ed: &Editor) -> f32 {
    3.0 * button_size(ed).x
}

/// The minimize, maximize and close buttons at the right end of the menu
/// bar. Call last inside the menu bar; the space before them stays empty,
/// so it moves the window.
pub fn window_buttons(ui: &mut Ui, ed: &mut Editor) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let size = button_size(ed);
        for kind in [Button::Close, Button::Maximize, Button::Minimize] {
            if window_button(ui, kind, size, ed.window_maximized).clicked() {
                ed.window_request = Some(match kind {
                    Button::Close => WindowRequest::Close,
                    Button::Maximize => WindowRequest::ToggleMaximize,
                    Button::Minimize => WindowRequest::Minimize,
                });
            }
        }
    });
}

#[derive(Clone, Copy, PartialEq)]
enum Button {
    Minimize,
    Maximize,
    Close,
}

fn window_button(ui: &mut Ui, kind: Button, size: egui::Vec2, maximized: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let label = match kind {
        Button::Minimize => "Minimize",
        Button::Maximize if maximized => "Restore",
        Button::Maximize => "Maximize",
        Button::Close => "Close",
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let v = ui.visuals();
    let hot = resp.hovered() || resp.is_pointer_button_down_on();
    if hot {
        let fill = if kind == Button::Close {
            if resp.is_pointer_button_down_on() {
                Color32::from_rgb(160, 30, 22)
            } else {
                Color32::from_rgb(196, 43, 28)
            }
        } else {
            v.widgets.hovered.weak_bg_fill
        };
        ui.painter().rect_filled(rect, 0.0, fill);
    }
    let ink = if hot && kind == Button::Close { Color32::WHITE } else { v.text_color() };
    let stroke = Stroke::new(1.0, ink);
    let c = rect.center();
    let h = 5.0;
    let p = ui.painter();
    match kind {
        Button::Minimize => {
            p.line_segment([c + vec2(-h, 0.5), c + vec2(h, 0.5)], stroke);
        }
        Button::Maximize if maximized => {
            // Restore: two overlapping squares.
            let back = Rect::from_min_size(c + vec2(-h + 2.0, -h), vec2(2.0 * h - 2.0, 2.0 * h - 2.0));
            let front = Rect::from_min_size(c + vec2(-h, -h + 2.0), vec2(2.0 * h - 2.0, 2.0 * h - 2.0));
            p.line_segment([back.left_top(), back.right_top()], stroke);
            p.line_segment([back.right_top(), back.right_bottom()], stroke);
            p.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Middle);
        }
        Button::Maximize => {
            p.rect_stroke(Rect::from_center_size(c, vec2(2.0 * h, 2.0 * h)), 0.0, stroke, egui::StrokeKind::Middle);
        }
        Button::Close => {
            p.line_segment([c + vec2(-h, -h), c + vec2(h, h)], stroke);
            p.line_segment([c + vec2(-h, h), c + vec2(h, -h)], stroke);
        }
    }
    resp.on_hover_text(label)
}

/// What a press, drag or click on the empty part of the bar asks for.
pub fn bar_background(resp: &egui::Response, ed: &mut Editor) {
    if resp.double_clicked() {
        ed.window_request = Some(WindowRequest::ToggleMaximize);
    } else if resp.drag_started_by(egui::PointerButton::Primary) {
        ed.window_request = Some(WindowRequest::Drag);
    } else if resp.secondary_clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        ed.window_request = Some(WindowRequest::Menu(pos));
    }
}

/// Thin areas along the window border that resize it, above everything
/// else so a press there never reaches a panel or starts a stroke. Not
/// shown while the window is maximized.
pub fn resize_edges(ui: &mut Ui, ed: &mut Editor) {
    if ed.window_maximized {
        return;
    }
    let ctx = ui.ctx().clone();
    let screen = ctx.content_rect();
    let strips = [
        ("edge_n", Rect::from_min_max(screen.left_top(), Pos2::new(screen.right(), screen.top() + EDGE))),
        ("edge_s", Rect::from_min_max(Pos2::new(screen.left(), screen.bottom() - EDGE), screen.right_bottom())),
        ("edge_w", Rect::from_min_max(screen.left_top(), Pos2::new(screen.left() + EDGE, screen.bottom()))),
        ("edge_e", Rect::from_min_max(Pos2::new(screen.right() - EDGE, screen.top()), screen.right_bottom())),
    ];
    for (id, rect) in strips {
        egui::Area::new(egui::Id::unique(id)).fixed_pos(rect.min).order(egui::Order::Foreground).constrain(false).show(
            &ctx,
            |ui| {
                let (_, resp) = ui.allocate_exact_size(rect.size(), Sense::drag());
                let Some(pos) = resp.hover_pos().or_else(|| resp.interact_pointer_pos()) else { return };
                let Some(edge) = edge_at(screen, pos) else { return };
                ctx.set_cursor_icon(edge.cursor());
                if resp.is_pointer_button_down_on() && ctx.input(|i| i.pointer.primary_pressed()) {
                    ed.window_request = Some(WindowRequest::Resize(edge));
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_and_corners_are_found_near_the_border_only() {
        let s = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        assert_eq!(edge_at(s, Pos2::new(400.0, 300.0)), None);
        assert_eq!(edge_at(s, Pos2::new(400.0, 1.0)), Some(Edge::North));
        assert_eq!(edge_at(s, Pos2::new(400.0, 598.0)), Some(Edge::South));
        assert_eq!(edge_at(s, Pos2::new(2.0, 300.0)), Some(Edge::West));
        assert_eq!(edge_at(s, Pos2::new(798.0, 300.0)), Some(Edge::East));
        assert_eq!(edge_at(s, Pos2::new(2.0, 10.0)), Some(Edge::NorthWest));
        assert_eq!(edge_at(s, Pos2::new(10.0, 2.0)), Some(Edge::NorthWest));
        assert_eq!(edge_at(s, Pos2::new(798.0, 590.0)), Some(Edge::SouthEast));
        assert_eq!(edge_at(s, Pos2::new(10.0, 10.0)), None, "near a corner but off the border");
        assert_eq!(edge_at(s, Pos2::new(900.0, 1.0)), None);
    }
}
