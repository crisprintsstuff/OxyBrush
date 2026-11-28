use crate::editor::Editor;
use crate::settings::PanelSide;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockMenu {
    Materials,
    Pieces,
    Mesh,
    Tools,
    Groups,
    Mask,
    View,
}

impl DockMenu {
    pub const ALL: [Self; 7] =
        [Self::Materials, Self::Pieces, Self::Mesh, Self::Tools, Self::Groups, Self::Mask, Self::View];

    pub fn label(self) -> &'static str {
        match self {
            Self::Materials => "Materials",
            Self::Pieces => "Pieces",
            Self::Mesh => "Mesh",
            Self::Tools => "Tools",
            Self::Groups => "Groups",
            Self::Mask => "Mask",
            Self::View => "View",
        }
    }
}

pub type MenuDocks = [Option<PanelSide>; DockMenu::ALL.len()];

pub fn place(ctx: &egui::Context, ed: &mut Editor, menu: DockMenu, side: PanelSide) {
    ed.menu_docks[menu as usize] = Some(side);
    ed.show_panels = true;
    ctx.data_mut(|data| data.insert_temp(reveal_id(menu), true));
    ctx.request_repaint();
}

fn reveal_id(menu: DockMenu) -> egui::Id {
    egui::Id::unique(("reveal_docked_menu", menu as usize))
}

pub fn take_reveal(ui: &egui::Ui, menu: DockMenu) -> bool {
    ui.data_mut(|data| data.remove_temp::<bool>(reveal_id(menu))).unwrap_or(false)
}

pub fn drag_handle(ui: &mut egui::Ui, menu: DockMenu, caption: &str) {
    let response = ui
        .add(egui::Button::new(egui::RichText::new(caption).strong()).frame(false).sense(egui::Sense::drag()))
        .on_hover_cursor(egui::CursorIcon::Grab);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Drag {} to dock", menu.label())));
    response.dnd_set_drag_payload(menu);
    crate::tips::hover(response, "Drag this header to the left or right sidebar to dock the menu.");
}

pub fn place_buttons(ui: &mut egui::Ui, ed: &mut Editor, menu: DockMenu) {
    for (side, label) in [(PanelSide::Left, "Dock left"), (PanelSide::Right, "Dock right")] {
        if ui.small_button(label).clicked() {
            place(ui.ctx(), ed, menu, side);
            ui.close();
        }
    }
}

pub fn drop_targets(ctx: &egui::Context, ed: &mut Editor) {
    let Some(menu) = egui::DragAndDrop::payload::<DockMenu>(ctx) else { return };
    let rect = ctx.content_rect().shrink(8.0);
    let width = (rect.width() * 0.18).clamp(84.0, 200.0);
    let pointer = ctx.pointer_hover_pos();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("menu_dock_targets")));
    let visuals = ctx.global_style().visuals.clone();
    for (side, x, label) in
        [(PanelSide::Left, rect.left(), "Dock left"), (PanelSide::Right, rect.right() - width, "Dock right")]
    {
        let target =
            egui::Rect::from_min_size(egui::pos2(x, rect.top() + 36.0), egui::vec2(width, rect.height() - 36.0));
        let hovered = pointer.is_some_and(|pos| target.contains(pos));
        let fill = if hovered { visuals.selection.bg_fill } else { visuals.panel_fill };
        painter.rect(target, 6.0, fill, visuals.selection.stroke, egui::StrokeKind::Inside);
        painter.text(
            target.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::TextStyle::Button.resolve(&ctx.global_style()),
            if hovered { visuals.selection.stroke.color } else { visuals.text_color() },
        );
        if hovered && ctx.input(|input| input.pointer.primary_released()) {
            egui::DragAndDrop::clear_payload(ctx);
            place(ctx, ed, *menu, side);
            egui::Popup::close_all(ctx);
            ctx.request_repaint();
        }
    }
}
