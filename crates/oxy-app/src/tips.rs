//! Hover text for every control, in one style.
//!
//! A tooltip is one short explanation of what the control does. When that
//! action has a shortcut, a second line reads `Shortcut: …`. The words of
//! that line come from [`Cmd::shortcut`]: the same label the menus and the
//! command palette show. Key dispatch still lives in `app::handle_key`.
//! When shortcuts become customizable, change [`Cmd::shortcut`] (the macro
//! below) to read the active binding and leave the call sites alone.
//!
//! A touch screen has no hover. Holding a finger still lets egui show the
//! tooltip under the finger. A long-press (egui's touch secondary click,
//! about 0.8 s, and only if the finger has not moved) pins the same text
//! under the control after the finger lifts, and does not activate it.

use crate::pose::{GizmoMode, Handle};
use crate::settings::{PanelSide, QuickBar, ShelfPosition, UpdateMode};
use egui::{Pos2, Response, Ui};
use oxy_render::MatcapPreset;
use oxy_sculpt::{BrushKind, Falloff};

/// One registered action: its palette/menu name, what it does, and the
/// shortcut label (empty when there is none).
macro_rules! commands {
    ($($variant:ident => $label:literal, $shortcut:literal, $what:literal);+ $(;)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Cmd {
            $($variant,)+
        }

        impl Cmd {
            /// Every registered command. The tooltip test walks this list.
            #[cfg_attr(not(test), allow(dead_code))]
            pub const ALL: &'static [Cmd] = &[$(Cmd::$variant,)+];

            /// Name shown in the command palette.
            pub fn label(self) -> &'static str {
                match self {
                    $(Cmd::$variant => $label,)+
                }
            }

            /// Shortcut label, or `""` when the action has none.
            ///
            /// This is the only place UI text should read a shortcut from.
            pub fn shortcut(self) -> &'static str {
                match self {
                    $(Cmd::$variant => $shortcut,)+
                }
            }

            /// What the action does, with no shortcut mixed in.
            pub fn what(self) -> &'static str {
                match self {
                    $(Cmd::$variant => $what,)+
                }
            }
        }
    };
}

commands! {
    Open => "Open…", "Ctrl+O", "Open a saved project or a mesh file.";
    Import => "Import mesh as piece…", "Ctrl+I", "Add an OBJ or STL file as a new piece.";
    Save => "Save", "Ctrl+S", "Save the project to its current file.";
    SaveAs => "Save as…", "Ctrl+Shift+S", "Save the project to a new file.";
    SendBack => "Send back to OxyCharacter", "", "Save to the file OxyCharacter opened, so it picks up your changes. Works even after Save As.";
    Export => "Export visible pieces…", "Ctrl+E", "Write the visible pieces to a file for other programs or for printing.";
    NewSphere => "New sphere", "", "Start over with a sphere. You are asked before unsaved work is discarded.";
    NewCube => "New cube", "", "Start over with a cube. You are asked before unsaved work is discarded.";
    NewPlane => "New plane", "", "Start over with a flat plane. You are asked before unsaved work is discarded.";
    Undo => "Undo", "Ctrl+Z", "Undo the last change.";
    Redo => "Redo", "Ctrl+Shift+Z", "Put back the last change you undid.";
    LastStrokeAsCurve => "Edit last stroke as curve", "", "Take back the stroke you just made and reopen its path as an editable curve with the same brush: drag its points, change size and strength along it, then Enter applies it. Works on Freehand strokes, right after you make them.";
    LastStrokeAsCurveMove => "Use last stroke as Curve Move curve", "", "Take back the stroke you just made and use its path as a Curve Move curve, to bend and move the part along it.";
    LastStrokeAsTube => "Use last stroke as tube path", "", "Take back the stroke you just made and draw a tube along its path, added to the same piece. Shape it, then Enter adds it.";
    UndoPiece => "Undo on this piece", "Ctrl+Alt+Z", "Undo the last change to the active piece only, even if you changed other pieces since.";
    RedoPiece => "Redo on this piece", "Ctrl+Alt+Shift+Z", "Put back the last change to the active piece that you undid, leaving other pieces as they are.";
    AddSphere => "Add sphere piece", "", "Add a sphere as a new piece.";
    AddCube => "Add cube piece", "", "Add a cube as a new piece.";
    AddPlane => "Add plane piece", "", "Add a flat plane as a new piece.";
    AddCylinder => "Add cylinder piece", "", "Add a cylinder as a new piece, with even triangles ready to sculpt.";
    AddCone => "Add cone piece", "", "Add a cone as a new piece, with even triangles ready to sculpt.";
    AddTorus => "Add torus piece", "", "Add a torus (ring) as a new piece.";
    DeformWindow => "Deform…", "", "Open the Deform window: bend, twist, taper, inflate, polish and more for the whole piece (or its unmasked part). Drag a bar to see it, let go to apply.";
    DeformFit => "Fit to standard size", "", "Centre the piece on the origin and scale it so its longest side is 2 units, keeping its proportions.";
    DeformRepeat => "Repeat on this piece", "", "Apply the last deformer again, with the same amount and axes.";
    DeformRepeatOthers => "Repeat on other pieces", "", "Apply the last deformer to every other visible piece, each around its own middle. One undo step.";
    Retopology => "Retopology (draw quads)", "", "Draw a new low-polygon mesh of quads by hand over the active piece: click to place points on its surface, Shift+click between them to make quads. The result is a new piece, saved with the project, and exports as quads.";
    AddTube => "Draw a tube", "", "Drag a path from the model or empty space to get a tube along it. Drag its points to shape it, Shift+drag a point to change its thickness, then Accept to add it as a new piece.";
    EditTube => "Edit tube", "", "Reopen the active piece's most recent tube to change its path, thickness, sides or caps. Accept replaces it, as one undo step. A tube that was sculpted, remeshed or regrouped since is an ordinary part of the mesh and can't be edited this way.";
    Duplicate => "Duplicate piece", "", "Make a copy of the active piece.";
    MirrorDuplicate => "Mirror duplicate piece", "", "Copy the active piece, flipped across X.";
    DeletePiece => "Delete piece", "", "Remove the active piece.";
    Solo => "Solo piece / show all", "Alt+S", "Show only the active piece, or show every piece again.";
    Merge => "Merge and remesh visible pieces", "", "Fuse all visible pieces into the active one with a voxel remesh: one closed surface with new, even triangles. Use Join to keep the pieces as they are.";
    Join => "Join visible pieces", "", "Combine the active piece and the other visible pieces into one piece, exactly as they are: no remesh. Each piece keeps its own face groups, so you can still hide or select it.";
    SplitGroups => "Split by face groups", "", "Turn each face group into its own piece.";
    SplitLoose => "Split into loose parts", "", "Turn each separate surface into its own piece.";
    Lod => "Make LOD chain", "", "Add three lighter copies, at 1/2, 1/4 and 1/8 of the triangles.";
    AddLevel => "Add subdivision level", "Ctrl+D", "Add a subdivision level so you can sculpt finer detail.";
    LevelUp => "Subdivision level up", "D", "Sculpt on the next finer subdivision level.";
    LevelDown => "Subdivision level down", "Shift+D", "Sculpt on the next coarser subdivision level.";
    DeleteHigher => "Delete higher levels", "", "Remove subdivision levels finer than the current one.";
    DeleteLower => "Delete lower levels", "", "Remove subdivision levels coarser than the current one.";
    Remesh => "Voxel remesh", "Ctrl+R", "Rebuild the active piece with clean, even triangles. Removes subdivision levels.";
    Decimate => "Decimate", "", "Reduce the triangle count while keeping the shape.";
    DeleteHidden => "Delete hidden faces", "", "Remove hidden faces from the mesh.";
    CaptureAlpha => "Capture alpha from view", "", "Make an alpha from what the viewport shows: the depth of the model in the middle of the view, nearest parts white, at the size chosen next to it (up to 4096 px). It is added to the alphas and selected; use it with Stamp strokes to copy that shape onto a surface.";
    ExportAlpha => "Export alpha…", "", "Save the selected alpha as a 16-bit grayscale PNG to edit in a 2D app. Load it back with Load image….";
    CloseHoles => "Close holes", "", "Fill every hole in this piece with a smooth patch of new faces, in a face group of its own. Useful before 3D printing or after cutting.";
    SymmetrizePosX => "Symmetrize: +X onto −X", "", "Make this piece symmetric: keep the +X half (where the axis gizmo's red X points), replace the other half with its mirror image, and weld the two along the middle. Uses the piece's symmetry centre. One undo step.";
    SymmetrizeNegX => "Symmetrize: −X onto +X", "", "Make this piece symmetric: keep the −X half (away from the axis gizmo's red X), replace the other half with its mirror image, and weld the two along the middle. Uses the piece's symmetry centre. One undo step.";
    SplitMasked => "Split masked", "", "Move the masked part of this piece into a new piece, and keep the unmasked part in this one. Invert the mask first to split off the unmasked part instead. Both parts come out unmasked.";
    SplitHidden => "Split hidden faces", "", "Move the hidden faces into a new piece of their own, and keep the visible faces in this one.";
    GroupVisible => "Group visible", "Ctrl+W", "Give the visible faces their own face group.";
    GroupFromMask => "Group from mask", "", "Give the unmasked faces their own face group.";
    GroupIslands => "Group by islands", "", "Give each separate surface its own face group.";
    ShowAll => "Show all faces", "Ctrl+Shift+click empty", "Show every face again.";
    InvertVisibility => "Invert visibility", "", "Swap which faces are hidden.";
    InvertMask => "Invert mask", "Ctrl+Shift+I", "Swap masked and unmasked areas. Also Ctrl+click on empty space.";
    ClearMask => "Clear mask", "Alt+M", "Remove the mask so the whole surface can be sculpted. Also Ctrl+drag a box around empty space.";
    ExtractMask => "Extract mask", "", "Create a new piece from the masked area, with optional thickness.";
    BlurMask => "Blur mask", "", "Soften the edge of the mask.";
    SharpenMask => "Sharpen mask", "", "Make the mask edge crisper, more on or off.";
    MaskCavities => "Mask cavities", "", "Mask creases, pores and other concave areas.";
    MaskPeaks => "Mask peaks", "", "Mask ridges, edges and other convex areas.";
    MaskBorder => "Mask border", "", "Mask the edge of the visible surface: the border of an open mesh, and the edge of what's left when part of it is hidden. The rest is unmasked. Grow the mask to widen it.";
    MaskGroupBorders => "Mask group borders", "", "Mask the lines where face groups meet. The rest is unmasked. Grow the mask to widen them.";
    ShowMask => "Show / hide mask", "M", "Show or hide the mask tint.";
    GizmoMove => "Transform: move", "T", "Move the active piece. Masked parts stay put. T cycles the gizmo.";
    GizmoRotate => "Transform: rotate", "", "Rotate the active piece. Masked parts stay put. T cycles the gizmo.";
    GizmoScale => "Transform: scale", "", "Scale the active piece. Masked parts stay put. T cycles the gizmo.";
    GizmoMirrorX => "Mirror X", "", "Flip the active piece across the gizmo's X plane, through the pivot. Works on the whole piece (no mask).";
    GizmoMirrorY => "Mirror Y", "", "Flip the active piece across the gizmo's Y plane, through the pivot. Works on the whole piece (no mask).";
    GizmoMirrorZ => "Mirror Z", "", "Flip the active piece across the gizmo's Z plane, through the pivot. Works on the whole piece (no mask).";
    GizmoGround => "Put on the ground", "", "Move the active piece (its unmasked part) straight up or down so its lowest point is at Y = 0.";
    GizmoCenter => "Centre at origin", "", "Move the active piece (its unmasked part) so its bounding box is centred on the world origin.";
    FillColor => "Fill color", "", "Paint the current color onto everything that is not masked.";
    FillCavities => "Fill cavities with color", "", "Paint creases and pores, more where they are deeper.";
    FillPeaks => "Fill peaks with color", "", "Paint ridges and edges, for example for worn highlights.";
    ClearColor => "Clear color", "", "Remove vertex color. Masked areas keep theirs.";
    SymX => "Toggle X symmetry", "X", "Mirror strokes left and right.";
    SymY => "Toggle Y symmetry", "", "Mirror strokes up and down.";
    SymZ => "Toggle Z symmetry", "", "Mirror strokes front and back.";
    RaiseCarve => "Raise / carve", "", "Switch between adding clay and carving it away.";
    Frame => "Frame scene", "F", "Fit the whole scene in the view.";
    Front => "Front view", "Numpad 1", "Look at the front of the model.";
    Right => "Right view", "Numpad 3", "Look at the right side of the model.";
    Top => "Top view", "Numpad 7", "Look down at the model.";
    Ortho => "Toggle orthographic", "O", "Switch between a perspective view and an orthographic one.";
    GroupColors => "Face group colors", "G", "Show face groups as colors, or the normal material.";
    Shading => "Shading on / off", "", "Turn ambient occlusion and cavity shading on or off.";
    FlatShading => "Flat shading on / off", "", "Show each polygon as a flat facet instead of a smooth surface.";
    DynDetail => "Dynamic detail on / off", "", "Sculpting brushes add triangles where they need more detail and remove ones that have become too small, so you can build a shape from a simple sphere without remeshing.";
    LightViewport => "Light the viewport on / off", "", "Shade the model with lights (and shadows) instead of the material. Set them up in View > Shading > Lights.";
    LightsWindow => "Lights…", "", "Open the Lights window: add, move and color lights, and turn on shadows.";
    FloorGrid => "Floor grid on / off", "", "A grid on the floor under the model. It stays at the bottom of the model and its spacing follows the model's size.";
    Wireframe => "Wireframe on / off", "Shift+W", "Overlay polygon edges atop the shaded surface.";
    WireGroups => "Wireframe + groups on / off", "", "Toggle wireframe edges and face group colors together.";
    ResetWireframeColor => "Reset wireframe color", "", "Reset wireframe overlay lines to the default dark slate.";
    LoadMatcap => "Load matcap…", "", "Use a PNG or JPEG of a lit sphere as the material.";
    StrokeFromEmpty => "Strokes from empty space on / off", "", "A drag that starts off the model sculpts once it reaches the surface. Orbit with the middle mouse button or the view strip.";
    Panels => "Show / hide panels", "Tab", "Show or hide the side panels.";
    Palette => "Command palette", "Ctrl+K", "Type part of any command, brush, file or guide topic and run it.";
    FindBrush => "Find brush", "Space", "Type part of a brush name and pick it.";
    Shortcuts => "Shortcuts", "F1", "Show every keyboard and mouse shortcut.";
    Guide => "Guide", "F2", "Open the built-in guide. It works offline.";
    Welcome => "Welcome screen", "", "Open the welcome window.";
    CheckUpdates => "Check for updates…", "", "Look for a new version. Nothing about you or your work is sent.";
    CloseProject => "Close project", "", "Close this project and go back to the start screen. You are asked to save unsaved work first.";
    Quit => "Quit", "", "Close OxyBrush. You are asked before unsaved work is discarded.";
    Radius => "Brush radius", "[ and ]", "Brush size on screen, in points.";
    Strength => "Brush strength", "- and =", "How strong each dab is.";
    Raise => "Raise", "", "Brushes add clay. Hold Alt to carve for one stroke.";
    Carve => "Carve", "", "Brushes remove clay. Hold Alt to raise for one stroke.";
    Lasso => "Rectangle / lasso", "L", "Switch the selection between a rectangle and a lasso.";
    MaskGroup => "Mask face group", "Ctrl+M", "Mask the face group under the pointer.";
    QuickMenu => "Quick menu", "Q", "Hold Q, or right-click the view, for size, strength and Raise or Carve next to the cursor.";
    ClearGroups => "Clear groups", "", "Put every face back in the same group.";
    GrowVisible => "Grow visible", "", "Show one more ring of faces around what is already visible.";
    ShrinkVisible => "Shrink visible", "", "Hide one ring of faces around the edge of what is visible.";
    GrowMask => "Grow mask", "", "Spread the mask outward by one ring of vertices.";
    ShrinkMask => "Shrink mask", "", "Pull the mask inward by one ring of vertices.";
    SubdivideOnce => "Subdivide (single mesh)", "", "Smooth-subdivide the mesh once. This removes existing subdivision levels.";
    PaintBrush => "Paint brush", "", "Switch to the Paint brush and color the surface without moving it.";
    Back => "Back view", "", "Look at the back of the model.";
    Left => "Left view", "", "Look at the left side of the model.";
    Bottom => "Bottom view", "", "Look up from below the model.";
    FocalLengthWindow => "Focal length…", "", "Open the perspective lens window.";
    Focal18 => "Focal length 18 mm", "", "Set the perspective lens to 18 mm (ultra-wide angle).";
    Focal35 => "Focal length 35 mm", "", "Set the perspective lens to 35 mm (moderate wide angle).";
    Focal50 => "Focal length 50 mm", "", "Set the perspective lens to 50 mm (natural perspective).";
    Focal85 => "Focal length 85 mm", "", "Set the perspective lens to 85 mm (portrait lens).";
    Focal135 => "Focal length 135 mm", "", "Set the perspective lens to 135 mm (telephoto lens).";
    ResetFocal => "Reset focal length", "", "Restore the default perspective lens (about 38 mm, 35°).";
    BackgroundWindow => "Viewport background…", "", "Open the Theme window on its Colors tab, where the viewport background is set.";
    BackgroundGradient => "Viewport background: gradient", "", "Use a soft radial gradient for the viewport background.";
    BackgroundFlat => "Viewport background: flat", "", "Use a solid color for the viewport background.";
    ResetBackground => "Reset viewport background", "", "Restore the default gradient and viewport background colors.";
    ThemeWindow => "Theme…", "", "Open the Theme window: the theme, looks, accent color, viewport background, interface scale and corners, in one place.";
    WireframeWindow => "Wireframe settings…", "", "Open the wireframe color and opacity window.";
    AlignCursor => "Align cursor to surface on / off", "", "Draw the brush ring flat against the surface under the pointer.";
    Keymap => "Keymap", "", "Change keyboard shortcuts, mouse navigation and sculpt modifiers.";
    Preferences => "Preferences…", "Ctrl+,", "Open preferences to customize layout, appearance, plugins, keymap and updates.";
    RunScript => "Run Python script…", "", "Execute a Python script using the OxyBrush API.";
}

use crate::keymap::CommandId;

impl Cmd {
    /// Return the CommandId corresponding to this Cmd, if customizable in keymap.
    pub fn command_id(self) -> Option<CommandId> {
        Some(match self {
            Cmd::Open => CommandId::Open,
            Cmd::Import => CommandId::Import,
            Cmd::Save => CommandId::Save,
            Cmd::SaveAs => CommandId::SaveAs,
            Cmd::Export => CommandId::Export,
            Cmd::Undo => CommandId::Undo,
            Cmd::Redo => CommandId::Redo,
            Cmd::UndoPiece => CommandId::UndoPiece,
            Cmd::LastStrokeAsCurve => CommandId::LastStrokeAsCurve,
            Cmd::RedoPiece => CommandId::RedoPiece,
            Cmd::Palette => CommandId::CommandPalette,
            Cmd::FindBrush => CommandId::BrushFinder,
            Cmd::QuickMenu => CommandId::QuickMenu,
            Cmd::Radius => CommandId::RadiusUp,
            Cmd::Strength => CommandId::StrengthUp,
            Cmd::SymX => CommandId::SymmetryX,
            Cmd::Lasso => CommandId::SelectionLasso,
            Cmd::GizmoMove => CommandId::GizmoMove,
            Cmd::GizmoRotate => CommandId::GizmoRotate,
            Cmd::GizmoScale => CommandId::GizmoScale,
            Cmd::Frame => CommandId::Frame,
            Cmd::Front => CommandId::ViewFront,
            Cmd::Back => CommandId::ViewBack,
            Cmd::Left => CommandId::ViewLeft,
            Cmd::Right => CommandId::ViewRight,
            Cmd::Top => CommandId::ViewTop,
            Cmd::Bottom => CommandId::ViewBottom,
            Cmd::Ortho => CommandId::Orthographic,
            Cmd::GroupColors => CommandId::ShowGroups,
            Cmd::Wireframe => CommandId::Wireframe,
            Cmd::WireGroups => CommandId::WireGroups,
            Cmd::Panels => CommandId::TogglePanels,
            Cmd::Shortcuts => CommandId::Shortcuts,
            Cmd::ShowMask => CommandId::ShowMask,
            Cmd::ClearMask => CommandId::ClearMask,
            Cmd::InvertMask => CommandId::InvertMask,
            Cmd::MaskGroup => CommandId::MaskGroup,
            Cmd::Solo => CommandId::Solo,
            Cmd::AddLevel => CommandId::AddLevel,
            Cmd::LevelUp => CommandId::LevelUp,
            Cmd::LevelDown => CommandId::LevelDown,
            Cmd::Remesh => CommandId::Remesh,
            Cmd::GroupVisible => CommandId::GroupVisible,
            Cmd::Guide => CommandId::Guide,
            Cmd::Preferences => CommandId::Preferences,
            _ => return None,
        })
    }

    /// Primary shortcut label from the keymap if mapped, or default built-in shortcut.
    pub fn shortcut_in(self, km: &crate::keymap::Keymap) -> String {
        if let Some(id) = self.command_id() { km.primary(id) } else { self.shortcut().to_string() }
    }

    /// Full tooltip reading the shortcut dynamically from the keymap.
    pub fn tip_in(self, km: &crate::keymap::Keymap) -> String {
        format_tip(self.what(), &self.shortcut_in(km))
    }
}

impl Cmd {
    /// Explanation plus `Shortcut: …` when [`Self::shortcut`] is set.
    pub fn tip(self) -> String {
        format_tip(self.what(), self.shortcut())
    }
}

/// `what` on its own, or `what` plus a `Shortcut:` line.
pub fn format_tip(what: &str, shortcut: &str) -> String {
    if shortcut.is_empty() { what.to_owned() } else { format!("{what}\nShortcut: {shortcut}") }
}

/// Button label with the shortcut appended the way menus show it.
pub fn menu_text(name: &str, cmd: Cmd) -> String {
    let s = cmd.shortcut();
    if s.is_empty() { name.to_owned() } else { format!("{name}  ({s})") }
}

/// Brush name, what it does, and its shelf number key when it has one.
/// The number comes from the shelf order (`Editor::number_key`), not from
/// a fixed key.
pub fn brush_tip(kind: BrushKind, number_key: Option<&str>) -> String {
    let what = format!("{}. {}", kind.label(), crate::widgets::brush_description(kind));
    format_tip(&what, number_key.unwrap_or(""))
}

pub fn button_km(ui: &mut Ui, km: &crate::keymap::Keymap, name: &str, cmd: Cmd) -> Response {
    hover(ui.button(menu_text_km(km, name, cmd)), cmd.tip_in(km))
}

pub fn button_on_km(ui: &mut Ui, km: &crate::keymap::Keymap, enabled: bool, name: &str, cmd: Cmd) -> Response {
    hover(ui.add_enabled(enabled, egui::Button::new(menu_text_km(km, name, cmd))), cmd.tip_in(km))
}

pub fn check_km(ui: &mut Ui, km: &crate::keymap::Keymap, on: &mut bool, name: &str, cmd: Cmd) -> Response {
    hover(ui.checkbox(on, menu_text_km(km, name, cmd)), cmd.tip_in(km))
}

pub fn menu_text_km(km: &crate::keymap::Keymap, name: &str, cmd: Cmd) -> String {
    let s = cmd.shortcut_in(km);
    if s.is_empty() { name.to_owned() } else { format!("{name}  ({s})") }
}

pub fn button(ui: &mut Ui, name: &str, cmd: Cmd) -> Response {
    hover(ui.button(menu_text(name, cmd)), cmd.tip())
}

pub fn button_on(ui: &mut Ui, enabled: bool, name: &str, cmd: Cmd) -> Response {
    hover(ui.add_enabled(enabled, egui::Button::new(menu_text(name, cmd))), cmd.tip())
}

pub fn check(ui: &mut Ui, on: &mut bool, name: &str, cmd: Cmd) -> Response {
    hover(ui.checkbox(on, menu_text(name, cmd)), cmd.tip())
}

pub fn check_plain(ui: &mut Ui, on: &mut bool, label: &str, tip: &str) -> Response {
    hover(ui.checkbox(on, label), tip)
}

/// Top-level or nested menu whose button itself explains the menu.
pub fn menu(ui: &mut Ui, title: &str, tip: &str, add: impl FnOnce(&mut Ui)) {
    // Menus never grow past the window: a long one scrolls instead of
    // running off the screen.
    let max_height = (ui.ctx().content_rect().height() - 48.0).max(160.0);
    let inner = ui.menu_button(title, |ui| {
        egui::ScrollArea::vertical()
            .id_salt(("menu_scroll", title))
            .max_height(max_height)
            // Grow to fit the menu up to the window height before scrolling.
            .min_scrolled_height(max_height)
            .show(ui, add);
    });
    hover(inner.response, tip);
}

/// Attach the standard tooltip.
///
/// Mouse and pen: egui shows it on hover. Touch: a long-press pins the
/// same text below the control after the finger lifts.
pub fn hover(response: Response, text: impl Into<String>) -> Response {
    let text = text.into();
    if text.trim().is_empty() {
        return response;
    }
    let response = response.on_hover_text(text.clone()).on_disabled_hover_text(text.clone());
    if response.long_touched() || (response.secondary_clicked() && pen_hold(&response.ctx)) {
        let pos = response.rect.left_bottom() + egui::vec2(0.0, 4.0);
        // Read the clock before taking the data lock. `data_mut` holds that
        // lock, and `input` needs it too.
        let born = now(&response.ctx);
        response.ctx.data_mut(|d| d.insert_temp(pin_id(), PinnedTip { text, pos, fresh: true, born }));
    }
    response
}

fn pen_hold_id() -> egui::Id {
    egui::Id::unique("oxy_pen_hold")
}

/// The app turned a pen held still on a panel into a right-click this frame
/// (RFC 0015): the control under it pins its tooltip, as a long touch does.
pub fn mark_pen_hold(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(pen_hold_id(), true));
}

fn pen_hold(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(pen_hold_id())).unwrap_or(false)
}

/// Draw a tooltip that was pinned by a long-press. Call once per frame.
pub fn show_pinned(ctx: &egui::Context) {
    // The pen-hold mark lasts the frame that turned it into a right-click.
    ctx.data_mut(|d| {
        d.remove::<bool>(pen_hold_id());
    });
    let Some(mut tip) = ctx.data(|d| d.get_temp::<PinnedTip>(pin_id())) else { return };
    let (time, down, pressed) = ctx.input(|i| (i.time, i.pointer.any_down(), i.pointer.any_pressed()));
    if tip.fresh {
        tip.fresh = false;
        ctx.data_mut(|d| d.insert_temp(pin_id(), tip));
        return;
    }
    if pressed || time - tip.born > 4.0 {
        ctx.data_mut(|d| {
            d.remove::<PinnedTip>(pin_id());
        });
        return;
    }
    if down {
        return;
    }
    egui::Area::new(egui::Id::unique("oxy_pinned_tip_popup"))
        .order(egui::Order::Tooltip)
        .fixed_pos(tip.pos)
        .constrain(true)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(280.0);
                ui.label(&tip.text);
            });
        });
}

/// Tooltip pinned by the last long-press, if it is still stored.
#[cfg(test)]
pub fn pinned_text(ctx: &egui::Context) -> Option<String> {
    ctx.data(|d| d.get_temp::<PinnedTip>(pin_id()).map(|t| t.text))
}

fn pin_id() -> egui::Id {
    egui::Id::unique("oxy_pinned_tip")
}

fn now(ctx: &egui::Context) -> f64 {
    ctx.input(|i| i.time)
}

#[derive(Clone)]
struct PinnedTip {
    text: String,
    pos: Pos2,
    fresh: bool,
    born: f64,
}

/// A one-off explanation at `pos` (used for gizmo handles, which are not widgets).
pub fn show_at(ctx: &egui::Context, pos: Pos2, text: &str) {
    if text.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::unique("oxy_pointer_tip"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + egui::vec2(16.0, 16.0))
        .constrain(true)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(260.0);
                ui.label(text);
            });
        });
}

pub fn shelf_tip(p: ShelfPosition) -> &'static str {
    match p {
        ShelfPosition::Left => "Put the brush shelf along the left edge.",
        ShelfPosition::Bottom => "Put the brush shelf along the bottom edge.",
        ShelfPosition::Right => "Put the brush shelf along the right edge.",
    }
}

pub fn panel_tip(p: PanelSide) -> &'static str {
    match p {
        PanelSide::Right => "Put the settings panel on the right.",
        PanelSide::Left => "Put the settings panel on the left.",
    }
}

pub fn quick_bar_tip(q: QuickBar) -> &'static str {
    match q {
        QuickBar::ViewportTop => "Float size, strength and Raise/Carve at the top of the view.",
        QuickBar::ViewportBottom => "Float size, strength and Raise/Carve at the bottom of the view.",
        QuickBar::Toolbar => "Put size, strength and Raise/Carve in the toolbar.",
    }
}

pub fn update_tip(m: UpdateMode) -> &'static str {
    match m {
        UpdateMode::Off => "Do not contact the network, except when you click Check now.",
        UpdateMode::Notify => "Look for a new version about once a day and show a notice.",
        UpdateMode::Download => "Also download a new version in the background. Installing still waits for you.",
    }
}

pub fn falloff_tip(f: Falloff) -> &'static str {
    match f {
        Falloff::Smooth => "Soft edge: full strength in the middle, fading out.",
        Falloff::Sharp => "Strong in the center, then a quick drop.",
        Falloff::Linear => "Fades in a straight line from the center to the edge.",
        Falloff::Constant => "Full strength across the whole circle, then it stops.",
    }
}

pub fn stroke_tip(mode: crate::editor::StrokeMode) -> &'static str {
    use crate::editor::StrokeMode;
    match mode {
        StrokeMode::Freehand => "Dabs follow the pointer.",
        StrokeMode::Line => "Drag a straight line. The stroke is applied when you release.",
        StrokeMode::Stamp => "Drag out one dab. Distance sets the size, direction the rotation.",
        StrokeMode::Curve => {
            "Draw a stroke that stays editable: drag its points to reshape it, click the curve to add a point, Alt+click a point to remove it. Size, strength and the fades update it live; Enter or Accept applies it."
        }
    }
}

pub fn matcap_tip(m: MatcapPreset) -> &'static str {
    match m {
        MatcapPreset::Terracotta => "Warm red clay. The default look.",
        MatcapPreset::CrimsonClay => "Deep red clay with a soft specular sheen.",
        MatcapPreset::GreyClay => "Neutral grey, useful for reading the form.",
        MatcapPreset::Jade => "Green and glossy.",
        MatcapPreset::Pearl => "Pale and soft. Vertex color reads more truly.",
        MatcapPreset::Bronze => "Dark metal with a sharp highlight.",
    }
}

pub fn gizmo_mode_tip(m: GizmoMode) -> String {
    match m {
        GizmoMode::Move => Cmd::GizmoMove.tip(),
        GizmoMode::Rotate => Cmd::GizmoRotate.tip(),
        GizmoMode::Scale => Cmd::GizmoScale.tip(),
    }
}

pub fn gizmo_handle_tip(mode: GizmoMode, handle: Handle) -> &'static str {
    match (mode, handle) {
        (GizmoMode::Move, Handle::Axis(0)) => "Drag to move left and right. Masked parts stay put.",
        (GizmoMode::Move, Handle::Axis(1)) => "Drag to move up and down. Masked parts stay put.",
        (GizmoMode::Move, Handle::Axis(2)) => "Drag to move forward and back. Masked parts stay put.",
        (GizmoMode::Move, Handle::Center) => "Drag to move in the screen plane. Masked parts stay put.",
        (GizmoMode::Rotate, Handle::Axis(0)) => "Drag along the red ring to turn around the X axis. Shift snaps.",
        (GizmoMode::Rotate, Handle::Axis(1)) => "Drag along the green ring to turn around the Y axis. Shift snaps.",
        (GizmoMode::Rotate, Handle::Axis(2)) => "Drag along the blue ring to turn around the Z axis. Shift snaps.",
        (GizmoMode::Rotate, Handle::Center) => {
            "Drag along the outer ring to turn around the view direction. Shift snaps."
        }
        (GizmoMode::Scale, Handle::Axis(0)) => "Drag to scale left and right. Masked parts stay put.",
        (GizmoMode::Scale, Handle::Axis(1)) => "Drag to scale up and down. Masked parts stay put.",
        (GizmoMode::Scale, Handle::Axis(2)) => "Drag to scale forward and back. Masked parts stay put.",
        (GizmoMode::Scale, Handle::Center) => "Drag to scale evenly. Masked parts stay put.",
        (GizmoMode::Move, Handle::Plane(_)) => "Drag to move in this plane (along both of its axes). Shift snaps.",
        (GizmoMode::Scale, Handle::Plane(_)) => "Drag to scale along both axes of this plane. Shift snaps.",
        (GizmoMode::Rotate, Handle::Plane(_)) => "Drag to spin around the view direction.",
        (_, Handle::Axis(_)) => "Drag this handle. Masked parts stay put.",
    }
}

/// Which way the axis gizmo end faces: the camera will sit on that side.
pub fn axis_view_tip(dir: glam::Vec3) -> &'static str {
    let (axis, positive) = if dir.x.abs() >= dir.y.abs() && dir.x.abs() >= dir.z.abs() {
        ("X", dir.x > 0.0)
    } else if dir.y.abs() >= dir.z.abs() {
        ("Y", dir.y > 0.0)
    } else {
        ("Z", dir.z > 0.0)
    };
    match (axis, positive) {
        ("X", true) => "View from the right.",
        ("X", false) => "View from the left.",
        ("Y", true) => "View from above.",
        ("Y", false) => "View from below.",
        ("Z", true) => "View from the front.",
        _ => "View from the back.",
    }
}

pub fn alpha_tip(name: &str) -> String {
    if name == "None" {
        "No alpha: each dab is a plain circle.".into()
    } else {
        format!("{name}. A grayscale image that shapes each dab. White is full strength.")
    }
}

pub fn preset_tip(kind: &str, radius: f32, strength: f32, alpha: Option<&str>) -> String {
    match alpha {
        Some(a) => format!("{kind} brush, radius {radius:.0} pt, strength {strength:.2}, alpha {a}."),
        None => format!("{kind} brush, radius {radius:.0} pt, strength {strength:.2}."),
    }
}

pub fn recent_tip(path: &std::path::Path) -> String {
    format!("Open {}", path.display())
}

pub fn download_tip(dir: &std::path::Path) -> String {
    format!("Download the new version to {}. You still choose whether to install it.", dir.display())
}

pub fn swatch_tip(name: &str) -> String {
    format!("Paint with this {name} color.")
}

pub const SWATCH_NAMES: [&str; 8] = ["skin", "clay", "bone", "moss", "sea", "plum", "soot", "white"];

/// Explanations for controls that are not registered commands.
pub mod plain {
    pub const FILE: &str = "Projects and meshes: new, open, save, import and export.";
    pub const EDIT: &str = "Undo, redo, command palette, and preferences.";
    pub const PIECES: &str = "Add, copy, split and merge the meshes in the scene.";
    pub const MESH: &str = "Subdivision levels, remesh, decimate, color and the transform gizmo.";
    pub const GROUPS: &str = "Face groups, and which faces are hidden.";
    pub const MASK: &str = "Protect parts of the surface from brushes.";
    pub const VIEW: &str = "Camera, shading and materials.";
    pub const PLUGINS: &str = "Run Python scripts and custom plugins against the active document.";
    pub const RUN_SCRIPT: &str = "Pick and run a Python (.py) script using the OxyBrush API.";
    pub const OPEN_PLUGINS_DIR: &str = "Open the folder where OxyBrush looks for Python plugins.";
    pub const RELOAD_PLUGINS: &str = "Scan the plugins folder again for new or changed scripts.";
    pub const SETTINGS: &str = "Layout, where panels sit, scale, and update checks.";
    pub const HELP: &str = "The guide, shortcuts, and reporting a problem.";
    pub const NEW: &str = "Start a new scene from a sphere, cube or plane.";
    pub const ADD: &str = "Add a sphere, cube or plane as another piece.";
    pub const OPEN_RECENT: &str = "Files you opened, imported or saved recently.";
    pub const CLEAR_RECENT: &str = "Forget the recent-file list. The files themselves stay where they are.";
    pub const VERTEX_COLOR: &str = "Paint and fill vertex color. Masked areas are left alone.";
    pub const BUTTON_COLOR: &str = "Give buttons and other controls a color of your own instead of the theme's. Hovered buttons get a little lighter.";
    pub const BRUSH_ICON_COLOR: &str =
        "The color of the clay in the brush icons: the original terracotta, your accent color, or one of your own.";
    pub const SYMMETRIZE: &str = "Make this piece symmetric: keep one half, replace the other with its mirror image, and weld them along the middle. The axis gizmo's colored ends are the + sides.";
    pub const SYMMETRIZE_AXIS: &str =
        "Keep the first half named and mirror it onto the second, across the piece's symmetry centre. One undo step.";
    pub const TRANSFORM_MENU: &str = "Move, rotate or scale the active piece.";
    pub const ARRANGE: &str = "Where the brush shelf, the settings and the size bar sit.";
    pub const UPDATES: &str = "Whether OxyBrush looks for new versions. Off unless you turn it on.";
    pub const RESET_ORDER: &str = "Put the brushes back in the default order. Number keys follow that order.";
    pub const WELCOME_STARTUP: &str = "Show the welcome window the next time OxyBrush starts.";
    #[allow(dead_code)]
    pub const HIGH_CONTRAST: &str = "Stronger borders and brighter text.";
    pub const UI_SCALE: &str = "Make the whole interface larger or smaller.";
    pub const LAST_STROKE: &str = "Reopen the Freehand stroke you just made: as an editable curve, as a Curve Move curve, or as a tube path. The stroke is taken back first.";
    pub const DEFORM_AXIS: &str = "Which axes this deformer uses. Several can be on at once.";
    pub const DEFORM_MIRROR: &str =
        "Flip the whole piece across the X, Y or Z plane through its middle. Needs no mask and nothing hidden.";
    pub const DEFORM_USE_MASK: &str = "On: masked parts stay put and partly masked parts move partly. Off: the deformers move the whole visible piece.";
    pub const SLIDER_SIZE: &str = "How thick slider bars are. Thick bars are easier to see and to hit with a pen; the rest of the interface keeps its size.";
    pub const AUTOSAVE: &str = "How often a recovery copy is written, or Off to disable it.";
    pub const AO: &str = "Darkens places other parts of the surface shade, such as folds and the inside of ears.";
    pub const CAVITY: &str = "Darkens creases and pores and brightens edges, so fine detail reads clearly.";
    pub const LEVELS: &str = "Subdivision levels: sculpt big shapes on low levels and fine detail on high ones.";
    pub const TRIANGLES: &str = "Triangles in the active piece, then triangles in the whole scene.";
    #[allow(dead_code)]
    pub const TOUCH_SMOOTH: &str = "Every stroke smooths, the way holding Shift does. Tap again to turn it off.";
    #[allow(dead_code)]
    pub const TOUCH_MASK: &str = "Every stroke paints the mask, the way holding Ctrl does. Tap again to turn it off.";
    #[allow(dead_code)]
    pub const TOUCH_INVERT: &str =
        "Every stroke works the other way, the way holding Alt does. Tap again to turn it off.";
    pub const BRUSH_TILE: &str = "The current brush. Click to pick another.";
    pub const ALPHA_TILE: &str = "An image that shapes each dab. Click to pick one, or none.";
    pub const MATERIAL_TILE: &str = "How the clay is shaded. Click to pick a material.";
    pub const PANEL_DRAG_SCROLL: &str = "A pen reaches OxyBrush as a mouse without a wheel. With a pen (recognised by its pressure), dragging empty space in a panel scrolls it, as a finger does on a touch screen. Always also does it with a mouse, for pens whose drivers send no pressure. Never turns it off (touch screens still scroll).";
    pub const LOAD_IMAGE: &str = "Load a grayscale PNG. White is full strength, black is none.";
    pub const CAPTURE_SIZE: &str = "Size of the next captured alpha, in pixels. Larger sizes give smoother edges; 4096 takes a few seconds on a dense model.";
    pub const REMOVE_MATCAP: &str = "Remove this material from your library. The file is deleted.";
    pub const ORBIT: &str = "Drag to orbit. You can also drag empty space in the view.";
    pub const PAN: &str = "Drag to pan. You can also right-drag in the view.";
    pub const ZOOM: &str = "Drag up or down to zoom. You can also use the mouse wheel.";
    pub const AXIS_GIZMO: &str = "Click an axis end to view the model from that side, or drag to orbit.";
    pub const GIZMO_DONE: &str = "Hide the transform gizmo.";
    pub const GIZMO_SNAP: &str =
        "Snap gizmo drags to steps (set them in Pivot). Holding Shift while dragging snaps too.";
    pub const GIZMO_APPLY: &str =
        "Apply the typed values about the pivot, along the gizmo's axes, as one undo step (Enter in a field too).";
    pub const PLUGINS_MENU: &str = "Run installed plugins, install add-ons and manage plugins.";
    pub const GIZMO_MASKED: &str = "Masked parts stay put. A soft mask edge bends instead of tearing.";
    pub const PIECE_HIDE: &str = "Hide this piece. Hidden pieces are not sculpted or exported.";
    pub const PIECE_SHOW: &str = "Show this piece again.";
    pub const PIECE_RENAME: &str = "The active piece. Edit the name here.";
    pub const PIECE_ACTIVATE: &str = "Make this piece the one you sculpt. You can also Alt+click it in the view.";
    pub const PRESETS: &str = "Saved brushes: use one, save the current brush, or share it as a code or a file.";
    pub const PRESET_NAME: &str = "Name to save the current brush under.";
    pub const PRESET_SAVE: &str = "Save the current brush, including its size, strength, falloff and alpha.";
    pub const PRESET_CODE: &str = "Paste a brush code that starts with oxyb1:.";
    pub const PRESET_ADD: &str = "Add the pasted brush code to your presets.";
    pub const PRESET_IMPORT: &str = "Load a brush preset from an .oxbrush file.";
    pub const PRESET_COPY: &str = "Copy this brush as one line of text you can paste somewhere else.";
    pub const PRESET_EXPORT: &str = "Save this preset to an .oxbrush file.";
    pub const PRESET_DELETE: &str = "Delete this preset from your library.";
    pub const SPACING: &str = "Distance between dabs, as a fraction of the radius. Lower is smoother.";
    pub const AUTO_SMOOTH: &str = "Smooth a little after every dab.";
    pub const GRAB_THROUGH: &str =
        "Move everything under the brush circle, front to back, not just the surface you click.";
    pub const NOISE: &str = "Size of the noise pattern.";
    pub const CREASE_PINCH: &str =
        "How strongly the crease draws its groove walls together. 0 = carving only, 0.3 = default, 1 = tight pinch.";
    pub const PRESSURE_STRENGTH: &str = "A lighter pen touch makes a weaker dab.";
    pub const PRESSURE_RADIUS: &str = "A lighter pen touch makes a smaller brush.";
    pub const FRONT_FACES: &str = "Do not affect surfaces facing away from you, such as the back of a thin part.";
    pub const STABILIZER: &str =
        "The brush trails the pointer on a short string, for smoother lines. Zero turns it off.";
    pub const SPRAY: &str = "Scatter each dab's position, rotation and strength.";
    pub const LINK_SEND: &str = "These objects came from another app. Click to send them back: it updates the same objects (keeping their UVs and modifiers when you didn't change the topology), and adds pieces you made here as new objects.";
    pub const LINK_SEND_CLOSE: &str = "Send everything back to the other app, then close this document.";
    pub const LINK_BADGE: &str = "This document came from OxyCharacter. Click to send it back (File > Send back to OxyCharacter); OxyCharacter picks up the changes.";
    pub const HOLD_TO_CURVE: &str = "While drawing a Freehand stroke, hold the pen or mouse still for a moment: the stroke becomes an editable curve. Keep moving to extend it, then drag its points, and press Enter (or start the next stroke) to apply it.";
    pub const HOLD_TO_CURVE_ASK: &str =
        "Ask \"Edit this stroke as a curve?\" when you hold still, instead of switching right away.";
    pub const HOLD_YES: &str = "Turn the stroke into an editable curve (Enter).";
    pub const HOLD_NO: &str = "Keep the stroke as you drew it (Esc).";
    pub const CURVE_HELP: &str = "Drag a point to reshape the stroke, click the curve to add a point, Alt+click a point to remove it. Ctrl+Z undoes an edit.";
    pub const CURVE_CLOSED: &str =
        "Join the last point to the first, for rings and loops (needs three points; no fades).";
    pub const CURVE_SELECT: &str =
        "Click a point to set the size and strength there. Ctrl+Z undoes curve edits while it is open.";
    pub const CURVE_POINT_SIZE: &str =
        "Brush size at the selected point, relative to the Size slider. The stroke blends smoothly to the next points.";
    pub const CURVE_POINT_STRENGTH: &str = "Strength at the selected point, relative to the Strength slider.";
    pub const CURVE_FADE: &str = "How much of the curve, from that end, the stroke takes to reach full strength.";
    pub const CURVE_SIZE: &str = "Size, strength and the other brush settings update the stroke too.";
    pub const CURVE_ACCEPT: &str =
        "Apply the stroke (Enter). Starting another stroke also applies it. Undo removes it.";
    pub const CURVE_CANCEL: &str = "Close without changing the model (Esc).";
    pub const MOVE_HELP: &str = "Drag a point and the model around the curve follows, bending and stretching. Click the curve to add a point, Alt+click a point to remove it. Ctrl+Z undoes an edit.";
    pub const MOVE_SELECT: &str =
        "Click a point to set how far its pull reaches and how strongly. Masked areas never move.";
    pub const MOVE_POINT_REACH: &str =
        "How far from the curve the model follows at the selected point, relative to the Size slider.";
    pub const MOVE_POINT_STRENGTH: &str =
        "How fully the model follows at the selected point (100% moves it all the way).";
    pub const MOVE_SIZE: &str = "Size sets how far from the curve the model follows: make the circle cover the part's thickness so it moves as a whole. The falloff sets how softly the area beyond lets go.";
    pub const MOVE_RESET: &str = "Put every point back where you drew it, and the model with them.";
    pub const MOVE_ACCEPT: &str = "Apply the move (Enter). Starting another stroke also applies it. Undo removes it.";
    pub const MOVE_CENTRED: &str = "Put the curve through the middle of the part you draw over (an arm's core, a tail's spine), so it bends around its centre. Off: the curve stays on the surface.";
    pub const EXTRACT_PREVIEW: &str =
        "A preview of the shell made from the mask. Adjust it, then Accept to add it as a new piece.";
    pub const EXTRACT_THICKNESS: &str = "How thick the shell is. 0 makes an open surface with no thickness.";
    pub const EXTRACT_SMOOTH: &str = "Smooths the jagged edge of the mask before the shell is made.";
    pub const EXTRACT_ACCEPT: &str = "Add the shell as a new piece (Enter). Undo removes it.";
    pub const EXTRACT_CANCEL: &str = "Close without adding anything (Esc).";
    pub const PRESSURE_CURVE: &str =
        "Below 1 a light touch goes further. Above 1 the brush stays firm until you press harder.";
    pub const PRESSURE_MORE: &str = "The pen pressure curve. Click for more pen settings (Preferences > Pen & tablet).";
    pub const PEN_SETTINGS: &str =
        "How hard you press the pen and how strong the brush gets. Remembered between sessions.";
    pub const PRESSURE_PLOT: &str = "Pen pressure across, brush strength up. Drag the two points to shape the curve (an S shape gives fine control at both ends); double-click to straighten it. The dot shows your pen while it touches the tablet; the faint diagonal is a straight response.";
    pub const PRESSURE_S: &str =
        "Gentle at both the lightest and the hardest presses, quick in between: fine control at both ends.";
    pub const PRESSURE_SOFT: &str =
        "Light touches already sculpt, and full strength comes before you press all the way.";
    pub const PRESSURE_LINEAR: &str = "Brush strength follows the pen exactly.";
    pub const PRESSURE_FIRM: &str = "Light touches stay gentle; you need to press harder for full strength.";
    pub const PRESSURE_TRY: &str = "Press the pen on the tablet over the viewport to see its pressure on the curve.";
    pub const PRESSURE_MIN: &str = "Brush strength for the lightest touch, so a light stroke is never too faint.";
    pub const PRESSURE_FULL: &str =
        "How hard you press for full strength. Lower it if you have to press very hard to get a strong stroke.";
    pub const PRESSURE_DRIVER: &str = "Your tablet driver (Wacom, Huion, XP-Pen) may have its own pressure curve too. Keep that one straight and set the curve here, or the two add up.";
    pub const ALPHA_ROTATION: &str = "Turn the alpha image, in degrees.";
    pub const ALPHA_FOLLOW: &str = "Rotate the alpha to follow the direction of the stroke.";
    pub const DYN_DETAIL: &str = "How small the triangles get under the brush, relative to the brush size. A smaller brush also gives finer detail.";
    pub const DYN_LEVELS: &str = "Dynamic detail changes the triangles, which would remove subdivision levels, so it is off on pieces that have them.";
    pub const RADIAL: &str = "How many copies to repeat around the axis. Zero turns radial symmetry off.";
    pub const RADIAL_AXIS: &str = "The axis the radial copies go around.";
    pub const SYM_RECENTER: &str = "Put this piece's mirror planes through the middle of its shape. Use it after moving the whole piece, or if mirroring lands in the wrong place.";
    pub const SYM_WORLD: &str = "Put this piece's mirror planes back through the world origin (0, 0, 0).";
    pub const REMESH_RES: &str = "Voxel grid size for remeshing. Higher keeps more detail.";
    pub const KEEP: &str = "Share of triangles to keep when decimating.";
    pub const SELECTION_RECT: &str = "The selection is a rectangle.";
    pub const PAINT_COLOR: &str = "Color the Paint brush uses. Hold Alt while painting to erase.";
    pub const SECTION_PIECES: &str = "Meshes in this scene. The highlighted row is the one you sculpt.";
    pub const SECTION_BRUSH: &str = "Size, strength and the options for the current brush.";
    pub const SECTION_STROKE: &str = "How a drag becomes dabs: freehand, a straight line, or one stamp.";
    pub const SECTION_ALPHA: &str = "A grayscale image that shapes each dab.";
    pub const SECTION_SYMMETRY: &str = "Mirror a stroke to the other side, or repeat it around an axis.";
    pub const SECTION_TOPOLOGY: &str = "Subdivision levels, remesh and decimate.";
    pub const SECTION_GROUPS: &str = "Face groups, hiding, and the mask.";
    pub const SECTION_HISTORY: &str = "Recent steps. Dim rows are what redo puts back.";
    pub const REPORT_BUG: &str = "Open a pre-filled bug report in your browser.";
    pub const FEEDBACK: &str = "Open a feedback form in your browser.";
    pub const COPY_INFO: &str = "Copy the version, operating system and GPU, for a bug report.";
    pub const OPEN_LOG: &str = "Show the folder that contains the log file.";
    pub const TESTING_GUIDE: &str = "Open the testing guide on the web.";
    pub const CHANGELOG: &str = "Open the changelog on the web.";
    pub const ABOUT: &str = "Version, license, and the computer OxyBrush is running on.";
    pub const CHECK_NOW: &str = "Look for a new version now.";
    pub const SKIP_VERSION: &str = "Do not offer this version again.";
    pub const INSTALL: &str = "Install the downloaded version and restart.";
    pub const SHOW_FILE: &str = "Show the downloaded file so you can install it yourself.";
    pub const RELEASE_NOTES: &str = "Open this version's notes in your browser.";
    pub const REPO: &str = "Open the project page in your browser.";
    pub const UNSAVED_SAVE: &str = "Save the project, then continue.";
    pub const UNSAVED_DISCARD: &str = "Continue, and drop the changes since the last save.";
    pub const CANCEL: &str = "Go back without doing it.";
    pub const RECOVER: &str = "Open the autosaved work.";
    pub const KEYMAP_SETTINGS: &str = "Change keyboard shortcuts, mouse navigation and sculpt modifiers.";
    pub const SURFACE_CURSOR: &str = "Draw the brush ring flat on the surface under the pointer, at the brush size. Off the model it stays flat to the screen.";
    pub const FOCAL_LENGTH_MENU: &str = "Perspective lens in millimeters (35mm equivalent).";
    pub const BACKGROUND_RESET: &str = "Restore the built-in gradient and its colors.";
    pub const FOCAL_SLIDER: &str =
        "Shorter is wider, with more perspective. Longer flattens depth. The model stays the same size.";
    pub const FOCAL_RESET: &str = "Back to the default lens: 35° vertical, about 38.06 mm.";
    pub const LENS_STRIP: &str = "Focal length: perspective lens in millimeters. Orthographic view is unchanged.";
    pub const DISCARD_RECOVERY: &str = "Delete the autosave and start without it.";
    pub const LATER: &str = "Keep the autosave and ask again next time.";
    pub const START: &str = "Close this window and sculpt the current scene.";
    pub const SHOW_AT_STARTUP: &str = "Show this window every time OxyBrush starts.";
    pub const GUIDE_SEARCH: &str = "Find a topic. Words can be in the title or the text.";
    pub const GUIDE_TOPIC: &str = "Show this topic.";
    pub const PALETTE_SEARCH: &str = "Type part of a command, brush, file or topic. Enter runs it.";
    pub const UPDATE_BUTTON: &str = "Open the update details.";
    pub const FALLOFF: &str = "How the effect fades from the center of the brush to its edge.";
    pub const FOCAL_SHIFT: &str = "Moves the middle of the falloff. Left keeps more of the brush at full strength (firmer); right fades sooner (softer). The inner ring of the cursor follows it.";
    pub const STROKE_MODE: &str = "Freehand follows the pointer. Line draws straight. Stamp drags out one dab.";
    pub const ALPHA_MENU: &str = "Pick an alpha image, or none for a plain circle.";
    pub const AGENT_ENABLED: &str =
        "Let a program on this computer sculpt with you. Off by default. Nothing is sent off this machine.";
    pub const AGENT_COPY: &str = "Address and token, for your MCP client.";
    pub const AGENT_DISCONNECT: &str = "Disconnects the current agent.";
    pub const AGENT_CONNECTED_DISCONNECT: &str = "An AI agent on this computer is editing the scene. Click to disconnect it. Your work stays; the token changes.";

    #[cfg(test)]
    pub const ALL: &[&str] = &[
        FILE,
        EDIT,
        PIECES,
        MESH,
        GROUPS,
        MASK,
        VIEW,
        SETTINGS,
        HELP,
        NEW,
        ADD,
        OPEN_RECENT,
        CLEAR_RECENT,
        VERTEX_COLOR,
        BUTTON_COLOR,
        BRUSH_ICON_COLOR,
        SYMMETRIZE,
        SYMMETRIZE_AXIS,
        TRANSFORM_MENU,
        ARRANGE,
        UPDATES,
        RESET_ORDER,
        WELCOME_STARTUP,
        HIGH_CONTRAST,
        UI_SCALE,
        DEFORM_AXIS,
        LAST_STROKE,
        DEFORM_MIRROR,
        DEFORM_USE_MASK,
        SLIDER_SIZE,
        AUTOSAVE,
        AO,
        CAVITY,
        LEVELS,
        TRIANGLES,
        TOUCH_SMOOTH,
        TOUCH_MASK,
        TOUCH_INVERT,
        BRUSH_TILE,
        ALPHA_TILE,
        MATERIAL_TILE,
        LOAD_IMAGE,
        PANEL_DRAG_SCROLL,
        REMOVE_MATCAP,
        ORBIT,
        PAN,
        ZOOM,
        AXIS_GIZMO,
        GIZMO_DONE,
        GIZMO_SNAP,
        GIZMO_APPLY,
        PLUGINS_MENU,
        GIZMO_MASKED,
        PIECE_HIDE,
        PIECE_SHOW,
        PIECE_RENAME,
        PIECE_ACTIVATE,
        PRESETS,
        PRESET_NAME,
        PRESET_SAVE,
        PRESET_CODE,
        PRESET_ADD,
        PRESET_IMPORT,
        PRESET_COPY,
        PRESET_EXPORT,
        PRESET_DELETE,
        SPACING,
        AUTO_SMOOTH,
        GRAB_THROUGH,
        NOISE,
        PRESSURE_STRENGTH,
        PRESSURE_RADIUS,
        FRONT_FACES,
        STABILIZER,
        SPRAY,
        PRESSURE_CURVE,
        ALPHA_ROTATION,
        ALPHA_FOLLOW,
        RADIAL,
        RADIAL_AXIS,
        SYM_RECENTER,
        SYM_WORLD,
        REMESH_RES,
        KEEP,
        SELECTION_RECT,
        PAINT_COLOR,
        SECTION_PIECES,
        SECTION_BRUSH,
        SECTION_STROKE,
        SECTION_ALPHA,
        SECTION_SYMMETRY,
        SECTION_TOPOLOGY,
        SECTION_GROUPS,
        SECTION_HISTORY,
        REPORT_BUG,
        FEEDBACK,
        COPY_INFO,
        OPEN_LOG,
        TESTING_GUIDE,
        CHANGELOG,
        ABOUT,
        CHECK_NOW,
        SKIP_VERSION,
        INSTALL,
        SHOW_FILE,
        RELEASE_NOTES,
        REPO,
        UNSAVED_SAVE,
        UNSAVED_DISCARD,
        CANCEL,
        RECOVER,
        KEYMAP_SETTINGS,
        SURFACE_CURSOR,
        FOCAL_LENGTH_MENU,
        BACKGROUND_RESET,
        FOCAL_SLIDER,
        FOCAL_RESET,
        LENS_STRIP,
        DISCARD_RECOVERY,
        AGENT_ENABLED,
        AGENT_COPY,
        AGENT_DISCONNECT,
        AGENT_CONNECTED_DISCONNECT,
        LATER,
        START,
        SHOW_AT_STARTUP,
        GUIDE_SEARCH,
        GUIDE_TOPIC,
        PALETTE_SEARCH,
        UPDATE_BUTTON,
        FALLOFF,
        FOCAL_SHIFT,
        STROKE_MODE,
        ALPHA_MENU,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Choice;

    #[test]
    fn every_command_has_a_tooltip_that_reads_its_shortcut() {
        assert!(Cmd::ALL.len() > 40);
        for &cmd in Cmd::ALL {
            let what = cmd.what();
            assert!(what.len() > 12, "{cmd:?} ({})", cmd.label());
            assert!(!what.contains('\n'), "keep the shortcut on its own line: {cmd:?}");
            assert!(!what.contains("Shortcut"), "{cmd:?}");
            let tip = cmd.tip();
            let shortcut = cmd.shortcut();
            if shortcut.is_empty() {
                assert_eq!(tip, what, "{cmd:?}");
            } else {
                assert_eq!(tip, format!("{what}\nShortcut: {shortcut}"), "{cmd:?}");
                assert!(!shortcut.contains('\n'), "{cmd:?}");
            }
            assert_eq!(
                Cmd::ALL.iter().filter(|c| c.label() == cmd.label()).count(),
                1,
                "duplicate label {}",
                cmd.label()
            );
        }
    }

    #[test]
    fn every_brush_has_a_tooltip() {
        for kind in BrushKind::ALL {
            let bare = brush_tip(kind, None);
            assert!(bare.contains(kind.label()), "{bare}");
            assert!(bare.contains(crate::widgets::brush_description(kind)), "{bare}");
            assert!(!bare.contains("Shortcut:"), "{bare}");
            let keyed = brush_tip(kind, Some("4"));
            assert!(keyed.ends_with("\nShortcut: 4"), "{keyed}");
        }
    }

    #[test]
    fn plain_control_tips_are_sentences_without_a_hardcoded_shortcut() {
        assert!(!plain::ALL.is_empty());
        for tip in plain::ALL {
            assert!(tip.len() > 12, "{tip}");
            assert!(!tip.contains('\n'), "{tip}");
            assert!(!tip.contains("Shortcut"), "{tip}");
            assert!(!tip.contains("Ctrl+"), "read Ctrl shortcuts from Cmd::shortcut, not {tip}");
        }
    }

    #[test]
    fn choice_tips_cover_every_option() {
        for p in ShelfPosition::ALL {
            assert!(shelf_tip(*p).len() > 12);
        }
        for p in PanelSide::ALL {
            assert!(panel_tip(*p).len() > 12);
        }
        for q in QuickBar::ALL {
            assert!(quick_bar_tip(*q).len() > 12);
        }
        for m in UpdateMode::ALL {
            assert!(update_tip(*m).len() > 12);
        }
        for f in Falloff::ALL {
            assert!(falloff_tip(f).len() > 12);
        }
        for m in MatcapPreset::ALL {
            assert!(matcap_tip(m).len() > 12);
        }
        for m in crate::editor::StrokeMode::ALL {
            assert!(stroke_tip(m).len() > 12);
        }
        for m in GizmoMode::ALL {
            assert!(gizmo_mode_tip(m).contains(m.label()));
            for handle in [Handle::Axis(0), Handle::Axis(1), Handle::Axis(2), Handle::Center] {
                assert!(gizmo_handle_tip(m, handle).len() > 12);
            }
        }
    }

    fn raw(time: f64, events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0))),
            time: Some(time),
            events,
            ..Default::default()
        }
    }

    #[test]
    fn a_pen_hold_right_click_pins_the_tooltip_and_a_mouse_right_click_doesnt() {
        // RFC 0015: the app turns a pen held still into a right-click and
        // marks the frame; the control under it pins its tooltip.
        for (pen, expect) in [(true, true), (false, false)] {
            let ctx = egui::Context::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
            let mut rect = egui::Rect::NOTHING;
            let mut out = ctx.run_ui(raw(0.0, vec![]), |ui| {
                rect = ui.button("Go").rect;
            });
            out.textures_delta.clear();
            let pos = rect.center();
            let click = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Default::default(),
            };
            if pen {
                mark_pen_hold(&ctx);
            }
            let mut pinned = false;
            for (k, events) in
                [vec![egui::Event::PointerMoved(pos), click(true), click(false)], vec![]].into_iter().enumerate()
            {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        time: Some(0.1 + k as f64 * 0.1),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        hover(ui.button("Go"), "Does the thing.");
                        pinned |= ui.ctx().data(|d| d.get_temp::<PinnedTip>(pin_id())).is_some();
                        show_pinned(ui.ctx());
                    },
                );
                out.textures_delta.clear();
            }
            assert_eq!(pinned, expect, "pen hold: {pen}");
        }
    }

    #[test]
    fn long_press_pins_the_tooltip_and_does_not_click() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
        let mut rect = egui::Rect::NOTHING;
        let mut out = ctx.run_ui(raw(0.0, vec![]), |ui| {
            rect = ui.button("Go").rect;
        });
        out.textures_delta.clear();
        let pos = rect.center();
        let mut clicked = false;
        let mut t = 0.0_f64;
        let mut pressed = false;
        while t < 1.2 {
            t += 0.1;
            let events = if !pressed {
                pressed = true;
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                    egui::Event::Touch {
                        device_id: egui::TouchDeviceId(1),
                        id: egui::TouchId(1),
                        phase: egui::TouchPhase::Start,
                        pos,
                        force: None,
                    },
                ]
            } else {
                vec![egui::Event::PointerMoved(pos)]
            };
            let mut out = ctx.run_ui(
                egui::RawInput { screen_rect: Some(screen), time: Some(t), events, ..Default::default() },
                |ui| {
                    if hover(ui.button("Go"), "Does the thing.").clicked() {
                        clicked = true;
                    }
                    show_pinned(ui.ctx());
                },
            );
            out.textures_delta.clear();
        }
        assert!(!clicked, "a long-press explains the control and does not activate it");
        let pinned = pinned_text(&ctx).unwrap_or_default();
        assert!(pinned.contains("Does the thing"), "pinned tooltip was {pinned:?}");
    }
}
