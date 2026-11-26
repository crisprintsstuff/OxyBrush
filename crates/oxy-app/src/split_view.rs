//! Split view layout helpers ([RFC 0054](../../../docs/rfcs/0054-split-view.md)).

use glam::Vec2;

/// How many viewports the document canvas shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SplitLayout {
    #[default]
    Off,
    /// Two panes side by side (pane 0 left, pane 1 right).
    LeftRight,
    /// Two panes stacked (pane 0 top, pane 1 bottom).
    TopBottom,
}

impl SplitLayout {
    pub const ALL: [SplitLayout; 3] = [Self::Off, Self::LeftRight, Self::TopBottom];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::LeftRight => "Left / right",
            Self::TopBottom => "Top / bottom",
        }
    }

    pub fn is_split(self) -> bool {
        !matches!(self, Self::Off)
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "single" => Some(Self::Off),
            "left_right" | "leftright" | "horizontal" | "lr" => Some(Self::LeftRight),
            "top_bottom" | "topbottom" | "vertical" | "tb" => Some(Self::TopBottom),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::LeftRight => "left_right",
            Self::TopBottom => "top_bottom",
        }
    }
}

/// Gap between panes in physical pixels (divider hit area is wider).
pub const DIVIDER_PX: f32 = 4.0;

/// Pane rectangles in physical pixels `[x, y, w, h]` for `full` and `ratio`
/// (fraction for pane 0 along the split axis, clamped).
pub fn pane_rects(layout: SplitLayout, full: [f32; 4], ratio: f32) -> [[f32; 4]; 2] {
    let [x, y, w, h] = full;
    let ratio = ratio.clamp(0.15, 0.85);
    let gap = DIVIDER_PX;
    match layout {
        SplitLayout::Off => [full, full],
        SplitLayout::LeftRight => {
            let w0 = ((w - gap) * ratio).floor().max(1.0);
            let w1 = (w - gap - w0).max(1.0);
            [[x, y, w0, h], [x + w0 + gap, y, w1, h]]
        }
        SplitLayout::TopBottom => {
            let h0 = ((h - gap) * ratio).floor().max(1.0);
            let h1 = (h - gap - h0).max(1.0);
            [[x, y, w, h0], [x, y + h0 + gap, w, h1]]
        }
    }
}

/// Which pane contains `pos` (physical pixels), or `None` if outside `full`.
pub fn pane_at(layout: SplitLayout, full: [f32; 4], ratio: f32, pos: Vec2) -> Option<u8> {
    let [x, y, w, h] = full;
    if pos.x < x || pos.y < y || pos.x >= x + w || pos.y >= y + h {
        return None;
    }
    if !layout.is_split() {
        return Some(0);
    }
    let panes = pane_rects(layout, full, ratio);
    for (i, r) in panes.iter().enumerate() {
        if pos.x >= r[0] && pos.y >= r[1] && pos.x < r[0] + r[2] && pos.y < r[1] + r[3] {
            return Some(i as u8);
        }
    }
    // On the divider: pick the nearer pane.
    let panes = pane_rects(layout, full, ratio);
    let d0 = (pos - Vec2::new(panes[0][0] + panes[0][2] * 0.5, panes[0][1] + panes[0][3] * 0.5)).length_squared();
    let d1 = (pos - Vec2::new(panes[1][0] + panes[1][2] * 0.5, panes[1][1] + panes[1][3] * 0.5)).length_squared();
    Some(u8::from(d1 < d0))
}

/// Update `ratio` while dragging the divider; `pos` is the pointer in physical pixels.
pub fn drag_ratio(layout: SplitLayout, full: [f32; 4], pos: Vec2) -> f32 {
    let [x, y, w, h] = full;
    match layout {
        SplitLayout::Off => 0.5,
        SplitLayout::LeftRight => ((pos.x - x) / w.max(1.0)).clamp(0.15, 0.85),
        SplitLayout::TopBottom => ((pos.y - y) / h.max(1.0)).clamp(0.15, 0.85),
    }
}

/// Whether `pos` is on the divider hit strip.
pub fn on_divider(layout: SplitLayout, full: [f32; 4], ratio: f32, pos: Vec2) -> bool {
    if !layout.is_split() {
        return false;
    }
    let panes = pane_rects(layout, full, ratio);
    let hit = DIVIDER_PX * 2.0;
    match layout {
        SplitLayout::LeftRight => {
            let mid = panes[0][0] + panes[0][2];
            (pos.x - mid).abs() <= hit && pos.y >= full[1] && pos.y < full[1] + full[3]
        }
        SplitLayout::TopBottom => {
            let mid = panes[0][1] + panes[0][3];
            (pos.y - mid).abs() <= hit && pos.x >= full[0] && pos.x < full[0] + full[2]
        }
        SplitLayout::Off => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_right_panes_fill_the_canvas() {
        let full = [100.0, 50.0, 400.0, 300.0];
        let [a, b] = pane_rects(SplitLayout::LeftRight, full, 0.5);
        assert!((a[2] + DIVIDER_PX + b[2] - 400.0).abs() < 1.0);
        assert_eq!(a[1], 50.0);
        assert_eq!(b[0], a[0] + a[2] + DIVIDER_PX);
        assert_eq!(pane_at(SplitLayout::LeftRight, full, 0.5, Vec2::new(120.0, 100.0)), Some(0));
        assert_eq!(pane_at(SplitLayout::LeftRight, full, 0.5, Vec2::new(350.0, 100.0)), Some(1));
    }

    #[test]
    fn top_bottom_and_off() {
        let full = [0.0, 0.0, 200.0, 200.0];
        let [a, b] = pane_rects(SplitLayout::TopBottom, full, 0.4);
        assert!(a[3] < b[3] || (a[3] - b[3]).abs() < 80.0);
        assert_eq!(pane_at(SplitLayout::Off, full, 0.5, Vec2::new(10.0, 10.0)), Some(0));
        assert!(on_divider(SplitLayout::TopBottom, full, 0.5, Vec2::new(100.0, 100.0)));
    }
}
