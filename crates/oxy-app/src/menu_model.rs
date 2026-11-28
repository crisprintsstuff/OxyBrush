//! The menus as data, for the macOS menu bar (RFC 0028 phase 2).
//!
//! macOS shows an app's menus in the menu bar at the top of the screen,
//! drawn by the system, so they can't be egui widgets. This module lists
//! them as items with a label, enabled and checked state, in the same
//! order and with the same words as the in-window menus (`ui::draw`). Most
//! items are a [`Cmd`] and run the command palette's action for it, so a
//! command does the same thing from the palette, a shortcut or either
//! menu. Sliders and colour pickers can't go in a native menu: their items
//! open a window that holds them.
//!
//! The model is plain data and builds on every platform, so its tests run
//! on Linux; only `native_menu` (macOS) turns it into system menus.

use crate::editor::{Editor, ExportFormat};
use crate::tips::Cmd;
use crate::ui::UiAction;
use oxy_render::{InactiveLook, MatcapPreset};

/// What an item does when chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuId {
    /// The palette's action for this command.
    Cmd(Cmd),
    Recent(usize),
    ClearRecent,
    ExportFormat(ExportFormat),
    ExportScreenshot,
    Gltf(oxy_io::GltfCompression),
    /// Send back to the linked app; `true` also closes the document.
    SendBackLink(bool),
    UndoScene,
    RedoScene,
    /// Boolean of the active piece with piece `.0`.
    Boolean(u64, oxy_topo::BooleanOp),
    /// Project the active piece onto piece `.0` (RFC 0051 Phase 1).
    ProjectOnto(u64),
    /// Symmetrize along axis `.0` (0 X, 1 Y, 2 Z), keeping the positive side when `.1`.
    Symmetrize(usize, bool),
    Gizmo(crate::pose::GizmoMode),
    SubdivisionQuads,
    SubdivisionTriangles,
    SubdivisionSmooth,
    Inactive(InactiveLook),
    AlignCursor,
    HideCursorInStroke,
    MaterialForScene,
    MaterialForPiece,
    UseSceneMaterial,
    Matcap(MatcapPreset),
    CustomMatcap(usize),
    /// The Shading window: the sliders of View > Shading and Inactive pieces.
    ShadingWindow,
    MaterialsPanel,
    ReferencesWindow,
    Xray,
    XrayScope(oxy_render::XrayScope),
    RecipeBox,
    /// Run command `.1` of in-app plugin `.0`.
    Plugin(usize, usize),
    StopPlugin(usize),
    PluginPanel(usize),
    PythonPlugin(usize),
    InstallAddon,
    OpenPluginsFolder,
    ReloadPlugins,
    ManagePlugins,
    ReportBug,
    Feedback,
    CopySystemInfo,
    OpenLogFolder,
    TestingGuide,
    Changelog,
    About,
}

/// One entry of a menu.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Item(Item),
    Sub { label: String, enabled: bool, items: Vec<Node> },
    Separator,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: MenuId,
    pub label: String,
    pub enabled: bool,
    /// Some for an item shown with a check mark when on.
    pub checked: Option<bool>,
    /// The command whose shortcut the menu shows, if any.
    pub shortcut: Option<Cmd>,
}

/// A top-level menu: its title and entries.
#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    pub title: &'static str,
    pub items: Vec<Node>,
}

fn item(id: MenuId, label: impl Into<String>) -> Node {
    Node::Item(Item { id, label: label.into(), enabled: true, checked: None, shortcut: None })
}

/// A command, labelled as in the in-window menu.
fn cmd(c: Cmd, label: &str) -> Node {
    Node::Item(Item { id: MenuId::Cmd(c), label: label.into(), enabled: true, checked: None, shortcut: Some(c) })
}

fn cmd_on(c: Cmd, label: &str, enabled: bool) -> Node {
    with(cmd(c, label), |i| i.enabled = enabled)
}

fn cmd_check(c: Cmd, label: &str, on: bool) -> Node {
    with(cmd(c, label), |i| i.checked = Some(on))
}

fn check(id: MenuId, label: impl Into<String>, on: bool) -> Node {
    with(item(id, label), |i| i.checked = Some(on))
}

fn with(mut node: Node, f: impl FnOnce(&mut Item)) -> Node {
    if let Node::Item(i) = &mut node {
        f(i);
    }
    node
}

fn sub(label: impl Into<String>, items: Vec<Node>) -> Node {
    Node::Sub { label: label.into(), enabled: true, items }
}

/// The menus, in menu bar order, as they are now.
pub fn menus(ed: &Editor) -> Vec<Menu> {
    let busy = ed.job.is_some();
    vec![
        Menu { title: "File", items: file_menu(ed) },
        Menu { title: "Edit", items: edit_menu(ed) },
        Menu { title: "Pieces", items: pieces_menu(ed, busy) },
        Menu { title: "Mesh", items: mesh_menu(ed, busy) },
        Menu { title: "Tools", items: tools_menu(ed, busy) },
        Menu { title: "Groups", items: groups_menu() },
        Menu { title: "Mask", items: mask_menu(ed) },
        Menu { title: "View", items: view_menu(ed) },
        Menu { title: "Plugins", items: plugins_menu(ed, busy) },
        Menu { title: "Help", items: help_menu(ed) },
    ]
}

fn file_menu(ed: &Editor) -> Vec<Node> {
    let mut v = vec![
        sub(
            "New",
            vec![
                cmd(Cmd::NewSphere, "Sphere"),
                cmd(Cmd::NewCube, "Cube"),
                cmd(Cmd::NewPlane, "Plane"),
                cmd(Cmd::NewHumanFemale, "Human (female)"),
                cmd(Cmd::NewHumanMale, "Human (male)"),
                cmd(Cmd::NewEmpty, "Empty scene"),
            ],
        ),
        cmd(Cmd::NewTab, "New project"),
        cmd(Cmd::Open, "Open…"),
    ];
    let mut recent: Vec<Node> = ed
        .recent_files
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
            item(MenuId::Recent(i), name)
        })
        .collect();
    recent.push(Node::Separator);
    recent.push(item(MenuId::ClearRecent, "Clear list"));
    v.push(Node::Sub { label: "Open recent".into(), enabled: !ed.recent_files.is_empty(), items: recent });
    v.extend([
        cmd(Cmd::Import, "Import mesh as piece…"),
        Node::Separator,
        cmd(Cmd::Save, "Save"),
        cmd(Cmd::SaveAs, "Save as…"),
        cmd_on(Cmd::Revert, "Revert", ed.can_revert()),
        cmd(Cmd::CloseTab, "Close this project"),
    ]);
    if ed.send_back_available() {
        v.push(cmd(Cmd::SendBack, "Send back to OxyCharacter"));
    }
    if let Some(app) = ed.bridge.as_ref().map(|b| b.app().to_owned()) {
        v.push(item(MenuId::SendBackLink(false), format!("Send back to {app}")));
        v.push(item(MenuId::SendBackLink(true), format!("Send back to {app} and close")));
    }
    v.push(cmd_on(Cmd::Export, "Export visible…", ed.scene.pieces.iter().any(|p| p.visible)));
    v.push(sub(
        format!("Export type: .{}", ed.export_format.extension()),
        ExportFormat::ALL.iter().map(|&f| check(MenuId::ExportFormat(f), f.label(), ed.export_format == f)).collect(),
    ));
    v.push(sub(
        format!("glTF compression: {}", ed.gltf_compression.key()),
        oxy_io::GltfCompression::ALL
            .iter()
            .map(|&c| check(MenuId::Gltf(c), c.label(), ed.gltf_compression == c))
            .collect(),
    ));
    v.extend([
        item(MenuId::ExportScreenshot, "Export screenshot…"),
        Node::Separator,
        cmd(Cmd::RunScript, "Run Python script…"),
    ]);
    // Quit is in the application menu on macOS.
    v
}

fn edit_menu(ed: &Editor) -> Vec<Node> {
    let (undo, redo) = (ed.history.can_undo(), ed.history.can_redo());
    let reopen = ed.can_reopen_last_stroke();
    let replay = ed.can_replay_last_stroke();
    let mut v = vec![
        cmd_on(Cmd::Undo, "Undo", ed.can_undo()),
        cmd_on(Cmd::Redo, "Redo", ed.can_redo()),
        Node::Separator,
        cmd_on(Cmd::CopyPiece, "Copy piece", !ed.scene.pieces.is_empty()),
        cmd_on(Cmd::PastePiece, "Paste piece", !ed.piece_clipboard.is_empty()),
        sub(
            "Last stroke",
            vec![
                cmd_on(Cmd::ReplayLastStroke, "Replay", replay),
                cmd_on(Cmd::LastStrokeAsCurve, "Edit as curve", reopen),
                cmd_on(Cmd::LastStrokeAsCurveMove, "Use as Curve Move curve", reopen),
                cmd_on(Cmd::LastStrokeAsTube, "Use as tube path", reopen),
            ],
        ),
    ];
    if ed.undo_per_piece {
        v.push(with(item(MenuId::UndoScene, "Undo in whole scene"), |i| i.enabled = undo));
        v.push(with(item(MenuId::RedoScene, "Redo in whole scene"), |i| i.enabled = redo));
    } else {
        v.push(cmd_on(Cmd::UndoPiece, "Undo on this piece", undo));
        v.push(cmd_on(Cmd::RedoPiece, "Redo on this piece", redo));
    }
    v.extend([
        Node::Separator,
        cmd(Cmd::Palette, "Command palette…"),
        cmd(Cmd::FindBrush, "Find brush…"),
        // Preferences are Settings… in the application menu on macOS.
    ]);
    v
}

pub const MIRROR_DUPLICATE: &str = "Mirror duplicate";
pub const MIRROR_DUPLICATE_LINKED: &str = "Mirror duplicate linked";
pub const MIRROR_DUPLICATE_AXES: [(&str, Cmd); 3] =
    [("X", Cmd::MirrorDuplicate), ("Y", Cmd::MirrorDuplicateY), ("Z", Cmd::MirrorDuplicateZ)];
pub const MIRROR_DUPLICATE_LINKED_AXES: [(&str, Cmd); 3] =
    [("X", Cmd::MirrorDuplicateLinked), ("Y", Cmd::MirrorDuplicateLinkedY), ("Z", Cmd::MirrorDuplicateLinkedZ)];

fn pieces_menu(ed: &Editor, busy: bool) -> Vec<Node> {
    let mut v = vec![
        sub(
            "Add",
            vec![
                cmd(Cmd::AddSphere, "Sphere"),
                cmd(Cmd::AddCube, "Cube"),
                cmd(Cmd::AddPlane, "Plane"),
                cmd(Cmd::AddCylinder, "Cylinder"),
                cmd(Cmd::AddCone, "Cone"),
                cmd(Cmd::AddTorus, "Torus"),
                cmd(Cmd::AddDisc, "Disc"),
                cmd(Cmd::AddTube, "Tube (draw)…"),
                cmd(Cmd::AddSpheres, "OxySpheres…"),
                Node::Separator,
                cmd(Cmd::AddHumanFemale, "Human (female)"),
                cmd(Cmd::AddHumanMale, "Human (male)"),
            ],
        ),
        cmd_on(Cmd::EditTube, "Edit tube…", ed.can_edit_tube()),
        cmd_on(Cmd::EditSpheres, "Edit OxySpheres…", ed.can_edit_spheres()),
        cmd_on(Cmd::CopyPiece, "Copy piece", !ed.scene.pieces.is_empty()),
        cmd_on(Cmd::PastePiece, "Paste piece", !ed.piece_clipboard.is_empty()),
        cmd(Cmd::Duplicate, "Duplicate"),
        sub(MIRROR_DUPLICATE, MIRROR_DUPLICATE_AXES.into_iter().map(|(label, id)| cmd(id, label)).collect()),
        cmd(Cmd::DuplicateLinked, "Duplicate linked"),
        sub(
            MIRROR_DUPLICATE_LINKED,
            MIRROR_DUPLICATE_LINKED_AXES.into_iter().map(|(label, id)| cmd(id, label)).collect(),
        ),
        cmd_on(Cmd::Unlink, "Unlink copy", ed.active_link_source().is_some()),
        cmd_on(Cmd::DeletePiece, "Delete", ed.scene.pieces.len() > 1),
        cmd(Cmd::Solo, "Solo / show all"),
        Node::Separator,
        cmd(Cmd::SplitGroups, "Split by face groups"),
        cmd(Cmd::SplitLoose, "Split into loose parts"),
        cmd_on(Cmd::Lod, "Make LOD chain", !busy),
        Node::Separator,
    ];
    if ed.gizmo_pieces().len() > 1 {
        v.push(cmd_on(Cmd::JoinPicked, "Join picked", !busy));
    }
    v.push(cmd_on(Cmd::Join, "Join visible", !busy));
    v.push(cmd_on(Cmd::Merge, "Merge and remesh visible", !busy));
    v
}

fn mesh_menu(ed: &Editor, busy: bool) -> Vec<Node> {
    use oxy_mesh::multires::Scheme;
    let levels = ed.multires_level();
    let mut v = vec![
        cmd(Cmd::AddLevel, "Add level"),
        cmd_on(Cmd::LevelUp, "Level up", levels.is_some_and(|(a, c)| a + 1 < c)),
        cmd_on(Cmd::LevelDown, "Level down", levels.is_some_and(|(a, _)| a > 0)),
        cmd_on(Cmd::DeleteHigher, "Delete higher levels", levels.is_some_and(|(a, c)| a + 1 < c)),
        cmd_on(Cmd::DeleteLower, "Delete lower levels", levels.is_some_and(|(a, _)| a > 0)),
        cmd_on(Cmd::KeepLevel, "Keep this level only", levels.is_some()),
        cmd_on(Cmd::RebuildLowerLevel, "Rebuild lower level", ed.can_rebuild_lower_level()),
        Node::Separator,
        cmd(Cmd::SubdivideMasked, "Subdivide masked"),
        cmd(Cmd::SubdivideOnce, "Subdivide (single mesh)"),
    ];
    // The kind of new levels; a piece with levels keeps the kind it has.
    let fixed = levels.is_some();
    let scheme = if fixed { ed.level_scheme() } else { ed.subdivision_scheme };
    v.push(with(check(MenuId::SubdivisionQuads, "Makes quads", scheme == Scheme::CatmullClark), |i| {
        i.enabled = !fixed
    }));
    v.push(with(check(MenuId::SubdivisionTriangles, "Makes triangles", scheme == Scheme::Loop), |i| {
        i.enabled = !fixed
    }));
    v.push(check(MenuId::SubdivisionSmooth, "Smooth", ed.subdivision_smooth));
    v.extend([
        Node::Separator,
        cmd_on(Cmd::Remesh, "Voxel remesh", !busy),
        cmd_on(Cmd::OxyRemesh, "OxyRemesh", !busy),
        cmd_on(Cmd::Decimate, "Decimate", !busy),
        cmd_on(Cmd::ProjectDetail, "Project detail", !busy),
    ]);
    let active = ed.scene.try_active().map(|p| p.id);
    let project_onto: Vec<Node> = ed
        .scene
        .pieces
        .iter()
        .filter(|p| Some(p.id) != active)
        .map(|p| item(MenuId::ProjectOnto(p.id), p.name.clone()))
        .collect();
    v.push(Node::Sub {
        label: "Project onto…".into(),
        enabled: active.is_some() && !busy && !project_onto.is_empty(),
        items: project_onto,
    });
    let others: Vec<Node> = ed
        .scene
        .pieces
        .iter()
        .filter(|p| Some(p.id) != active)
        .map(|p| {
            use oxy_topo::BooleanOp as B;
            sub(
                format!("With {}", p.name),
                vec![
                    item(MenuId::Boolean(p.id, B::Difference), "Subtract it"),
                    item(MenuId::Boolean(p.id, B::Union), "Add it (union)"),
                    item(MenuId::Boolean(p.id, B::Intersection), "Keep only the overlap"),
                ],
            )
        })
        .collect();
    v.push(Node::Sub { label: "Boolean".into(), enabled: active.is_some() && !others.is_empty(), items: others });
    v.extend([
        cmd(Cmd::PrintTools, "3D print…"),
        cmd(Cmd::Diagnostics, "Topology diagnostics…"),
        Node::Separator,
        cmd(Cmd::DeleteHidden, "Delete hidden"),
        cmd(Cmd::CloseHoles, "Close holes"),
        cmd(Cmd::SplitHidden, "Split hidden"),
        cmd(Cmd::SplitMasked, "Split masked"),
    ]);
    v
}

fn tools_menu(ed: &Editor, busy: bool) -> Vec<Node> {
    use crate::pose::GizmoMode;
    let gizmo = GizmoMode::ALL
        .iter()
        .map(|&m| {
            let c = match m {
                GizmoMode::Combined => Cmd::GizmoAll,
                GizmoMode::Move => Cmd::GizmoMove,
                GizmoMode::Rotate => Cmd::GizmoRotate,
                GizmoMode::Scale => Cmd::GizmoScale,
            };
            with(check(MenuId::Gizmo(m), m.label(), ed.gizmo == Some(m)), |i| i.shortcut = Some(c))
        })
        .collect();
    let mut symmetrize = vec![
        cmd_on(Cmd::SymmetrizePosX, "+X onto −X", !busy),
        cmd_on(Cmd::SymmetrizeNegX, "−X onto +X", !busy),
        Node::Separator,
    ];
    for (axis, name) in [(1, "Y"), (2, "Z")] {
        for keep_positive in [true, false] {
            let (from, to) = if keep_positive { ("+", "−") } else { ("−", "+") };
            symmetrize.push(with(
                item(MenuId::Symmetrize(axis, keep_positive), format!("{from}{name} onto {to}{name}")),
                |i| i.enabled = !busy,
            ));
        }
    }
    vec![
        cmd(Cmd::DeformWindow, "Deform…"),
        cmd(Cmd::SurfaceNoiseWindow, "Surface Noise…"),
        cmd(Cmd::OxyMorph, "OxyMorph (Beta)…"),
        cmd(Cmd::ScaleCoverWindow, "Scale cover…"),
        sub("Transform", gizmo),
        sub("Symmetrize", symmetrize),
        Node::Separator,
        cmd(Cmd::Retopology, "Retopology (draw quads)…"),
        cmd(Cmd::PolyEdit, "Poly Edit (model polygons)…"),
        Node::Separator,
        sub(
            "Vertex color",
            vec![
                cmd(Cmd::PaintBrush, "Paint brush"),
                cmd(Cmd::ColorFilterWindow, "Color Filter…"),
                cmd_check(Cmd::ShowVertexColor, "Show vertex color", ed.view.show_vertex_color),
            ],
        ),
    ]
}

fn groups_menu() -> Vec<Node> {
    vec![
        cmd(Cmd::GroupVisible, "Group visible"),
        cmd(Cmd::GroupFromMask, "Group from mask"),
        cmd(Cmd::GroupIslands, "Group by islands"),
        cmd(Cmd::ClearGroups, "Clear groups"),
        Node::Separator,
        cmd(Cmd::ShowAll, "Show all"),
        cmd(Cmd::HideMasked, "Hide masked"),
        cmd(Cmd::InvertVisibility, "Invert visibility"),
        cmd(Cmd::GrowVisible, "Grow visible"),
        cmd(Cmd::ShrinkVisible, "Shrink visible"),
    ]
}

fn mask_menu(ed: &Editor) -> Vec<Node> {
    vec![
        cmd(Cmd::InvertMask, "Invert"),
        cmd(Cmd::FillMask, "Fill"),
        cmd(Cmd::ClearMask, "Clear"),
        cmd(Cmd::ExtractMask, "Extract mask…"),
        cmd(Cmd::SplitMasked, "Split masked"),
        cmd(Cmd::SubdivideMasked, "Subdivide masked"),
        Node::Separator,
        cmd(Cmd::BlurMask, "Blur"),
        cmd(Cmd::SharpenMask, "Sharpen"),
        cmd(Cmd::GrowMask, "Grow"),
        cmd(Cmd::ShrinkMask, "Shrink"),
        Node::Separator,
        cmd(Cmd::MaskCavities, "Mask cavities"),
        cmd(Cmd::MaskPeaks, "Mask peaks"),
        cmd(Cmd::MaskBorder, "Mask border"),
        cmd(Cmd::MaskGroupBorders, "Mask group borders"),
        Node::Separator,
        cmd_check(Cmd::ShowMask, "Show mask", ed.view.show_mask),
    ]
}

fn view_menu(ed: &Editor) -> Vec<Node> {
    let mut material = vec![check(MenuId::MaterialForScene, "For the whole scene", !ed.material_for_piece)];
    if let Some(name) = ed.scene.try_active().map(|p| p.name.clone()) {
        material.push(check(MenuId::MaterialForPiece, format!("For {name} only"), ed.material_for_piece));
        if ed.material_for_piece && ed.scene.try_active().is_some_and(|p| p.material.is_some()) {
            material.push(item(MenuId::UseSceneMaterial, "Use the scene's material"));
        }
    }
    material.push(Node::Separator);
    material.extend(MatcapPreset::ALL.iter().map(|&m| check(MenuId::Matcap(m), m.label(), ed.is_chosen_matcap(m))));
    material.extend(
        ed.custom_matcaps
            .iter()
            .enumerate()
            .map(|(i, m)| check(MenuId::CustomMatcap(i), m.name.clone(), ed.is_chosen_custom_matcap(i))),
    );
    material.push(cmd(Cmd::LoadMatcap, "Load matcap…"));
    let inactive = InactiveLook::ALL
        .iter()
        .map(|&look| check(MenuId::Inactive(look), look.label(), ed.view.inactive_look == look))
        .collect();
    vec![
        cmd(Cmd::Frame, "Frame scene"),
        cmd(Cmd::ZoomIn, "Zoom in"),
        cmd(Cmd::ZoomOut, "Zoom out"),
        sub(
            "Camera",
            vec![
                cmd(Cmd::Front, "Front"),
                cmd(Cmd::Back, "Back"),
                cmd(Cmd::Left, "Left"),
                cmd(Cmd::Right, "Right"),
                cmd(Cmd::Top, "Top"),
                cmd(Cmd::Bottom, "Bottom"),
                Node::Separator,
                cmd_check(Cmd::Ortho, "Orthographic", ed.camera.orthographic),
                cmd(Cmd::FocalLengthWindow, "Focal length…"),
            ],
        ),
        sub("X-ray", {
            let mut nodes = vec![check(MenuId::Xray, "Enable X-ray", ed.view.xray), Node::Separator];
            nodes.extend(
                oxy_render::XrayScope::ALL
                    .map(|scope| check(MenuId::XrayScope(scope), scope.label(), ed.view.xray_scope == scope)),
            );
            nodes
        }),
        item(MenuId::ReferencesWindow, "References…"),
        sub(
            "Shading",
            vec![
                item(MenuId::ShadingWindow, "Ambient occlusion, cavity and hard edges…"),
                cmd_check(Cmd::FlatShading, "Flat shading", ed.view.flat_shading),
                cmd_check(Cmd::ShowVertexColor, "Show vertex color", ed.view.show_vertex_color),
                cmd_check(Cmd::LightViewport, "Light the viewport", ed.view.lighting.enabled),
                cmd(Cmd::LightsWindow, "Lights…"),
                cmd_check(Cmd::FloorGrid, "Floor grid", ed.floor_grid),
                cmd_check(Cmd::Wireframe, "Wireframe", ed.view.wireframe),
                cmd(Cmd::WireframeWindow, "Wireframe settings…"),
                cmd_check(Cmd::GroupColors, "Face group colors", ed.view.show_groups),
            ],
        ),
        sub("Inactive pieces", inactive),
        sub(
            "Brush cursor",
            vec![
                check(MenuId::AlignCursor, "Align cursor to surface", ed.surface_cursor),
                check(MenuId::HideCursorInStroke, "Hide circle during stroke", ed.hide_cursor_during_stroke),
            ],
        ),
        item(MenuId::MaterialsPanel, "Materials panel…"),
        sub("Material", material),
        Node::Separator,
        cmd(Cmd::ThemeWindow, "Theme…"),
        cmd_check(Cmd::Panels, "Show panels", ed.show_panels),
        cmd_check(Cmd::SettingsPanel, "Show settings panel", ed.show_panels && ed.show_inspector),
        check(MenuId::RecipeBox, "Recipe Box", ed.show_parts_bin),
    ]
}

fn plugins_menu(ed: &Editor, busy: bool) -> Vec<Node> {
    let mut v = Vec::new();
    for (i, p) in ed.wasm_plugins.iter().enumerate() {
        let items = match &p.plugin {
            None => vec![item(MenuId::StopPlugin(i), "Stop (running…)")],
            Some(pl) => {
                let mut items: Vec<Node> = pl
                    .manifest
                    .commands
                    .iter()
                    .enumerate()
                    .map(|(k, c)| with(item(MenuId::Plugin(i, k), c.label.clone()), |it| it.enabled = !busy))
                    .collect();
                if !pl.manifest.params.is_empty() || pl.has_overlay() {
                    items.push(Node::Separator);
                    let label = if p.show_panel { "Hide panel" } else { "Panel (settings)…" };
                    items.push(item(MenuId::PluginPanel(i), label));
                }
                items
            }
        };
        v.push(sub(p.name.clone(), items));
    }
    if !ed.plugins.is_empty() {
        v.push(Node::Separator);
        v.extend(ed.plugins.iter().enumerate().map(|(i, p)| item(MenuId::PythonPlugin(i), format!("{}…", p.name))));
    }
    if !v.is_empty() {
        v.push(Node::Separator);
    }
    v.extend([
        item(MenuId::InstallAddon, "Install add-on…"),
        cmd(Cmd::RunScript, "Run Python script…"),
        item(MenuId::OpenPluginsFolder, "Open plugins folder"),
        item(MenuId::ReloadPlugins, "Reload plugins"),
        item(MenuId::ManagePlugins, "Manage plugins…"),
    ]);
    v
}

fn help_menu(ed: &Editor) -> Vec<Node> {
    let has_log = ed.system_info.log_path.as_ref().and_then(|p| p.parent()).is_some();
    vec![
        cmd(Cmd::Guide, "Guide"),
        cmd(Cmd::Palette, "Command palette"),
        cmd(Cmd::Welcome, "Welcome screen"),
        cmd(Cmd::Shortcuts, "Shortcuts"),
        Node::Separator,
        item(MenuId::ReportBug, "Report a bug…"),
        item(MenuId::Feedback, "Send feedback…"),
        item(MenuId::CopySystemInfo, "Copy system info"),
        with(item(MenuId::OpenLogFolder, "Open log folder"), |i| i.enabled = has_log),
        Node::Separator,
        cmd(Cmd::CheckUpdates, "Check for updates…"),
        item(MenuId::TestingGuide, "Testing guide (online)"),
        item(MenuId::Changelog, "Changelog"),
        // About OxyBrush is in the application menu on macOS.
    ]
}

/// Do what item `id` does. `ctx` opens web pages and the clipboard.
/// RETURNS: what the platform layer must still do (a file dialog), if
/// anything. A disabled item isn't chosen by the system, so this doesn't
/// check again.
pub fn activate(ed: &mut Editor, ctx: &egui::Context, id: MenuId) -> Option<UiAction> {
    use crate::support::REPO_URL;
    let open = |url: String| ctx.open_url(egui::OpenUrl::new_tab(url));
    match id {
        MenuId::Cmd(c) => return crate::palette::run_cmd(ed, c).flatten(),
        MenuId::Recent(i) => {
            if let Some(p) = ed.recent_files.get(i).cloned() {
                ed.open_recent(&p);
            }
        }
        MenuId::ClearRecent => ed.recent_files.clear(),
        MenuId::ExportFormat(f) => ed.export_format = f,
        MenuId::ExportScreenshot => ed.open_screenshot_export(),
        MenuId::Gltf(c) => ed.gltf_compression = c,
        MenuId::SendBackLink(close) => ed.send_back_link(close),
        MenuId::UndoScene => ed.undo_scene(),
        MenuId::RedoScene => ed.redo_scene(),
        MenuId::Boolean(piece, op) => ed.boolean_operation(piece, op),
        MenuId::ProjectOnto(piece) => ed.project_detail_onto(Some(piece)),
        MenuId::Symmetrize(axis, keep_positive) => ed.symmetrize(axis, keep_positive),
        MenuId::Gizmo(m) => ed.toggle_gizmo(m),
        MenuId::SubdivisionQuads => ed.subdivision_scheme = oxy_mesh::multires::Scheme::CatmullClark,
        MenuId::SubdivisionTriangles => ed.subdivision_scheme = oxy_mesh::multires::Scheme::Loop,
        MenuId::SubdivisionSmooth => ed.subdivision_smooth = !ed.subdivision_smooth,
        MenuId::Inactive(look) => ed.view.inactive_look = look,
        MenuId::AlignCursor => {
            ed.surface_cursor = !ed.surface_cursor;
            if !ed.surface_cursor {
                ed.forget_cursor_plane();
            }
        }
        MenuId::HideCursorInStroke => ed.hide_cursor_during_stroke = !ed.hide_cursor_during_stroke,
        MenuId::MaterialForScene => ed.material_for_piece = false,
        MenuId::MaterialForPiece => ed.material_for_piece = true,
        MenuId::UseSceneMaterial => ed.set_piece_material(None),
        MenuId::Matcap(m) => ed.choose_matcap(m),
        MenuId::CustomMatcap(i) => {
            if i < ed.custom_matcaps.len() {
                ed.choose_custom_matcap(i);
            }
        }
        MenuId::Xray => ed.view.xray = !ed.view.xray,
        MenuId::XrayScope(scope) => ed.view.xray_scope = scope,
        MenuId::ReferencesWindow => ed.show_references = true,
        MenuId::ShadingWindow => ed.show_shading = true,
        MenuId::MaterialsPanel => crate::ui::show_materials_panel(ctx, ed),
        MenuId::RecipeBox => ed.show_parts_bin = !ed.show_parts_bin,
        MenuId::Plugin(i, k) => ed.run_wasm_plugin(i, k),
        MenuId::StopPlugin(i) => ed.stop_wasm_plugin(i),
        MenuId::PluginPanel(i) => {
            if let Some(p) = ed.wasm_plugins.get_mut(i) {
                p.show_panel = !p.show_panel;
            }
        }
        MenuId::PythonPlugin(i) => ed.request_run_plugin = ed.plugins.get(i).map(|p| p.path.clone()),
        MenuId::InstallAddon => ed.request_install_addon = true,
        MenuId::OpenPluginsFolder => ed.request_open_plugins_dir = true,
        MenuId::ReloadPlugins => ed.request_reload_plugins = true,
        MenuId::ManagePlugins => ed.open_settings(crate::editor::SettingsTab::Plugins),
        MenuId::ReportBug => open(ed.system_info.bug_report_url()),
        MenuId::Feedback => open(ed.system_info.feedback_url()),
        MenuId::CopySystemInfo => {
            ctx.copy_text(ed.system_info.text());
            ed.status = "System info copied to the clipboard".into();
        }
        MenuId::OpenLogFolder => {
            if let Some(dir) = ed.system_info.log_path.as_ref().and_then(|p| p.parent()) {
                crate::support::open_folder(dir);
            }
        }
        MenuId::TestingGuide => open(format!("{REPO_URL}/blob/main/docs/TESTING_GUIDE.md")),
        MenuId::Changelog => open(format!("{REPO_URL}/blob/main/CHANGELOG.md")),
        MenuId::About => ed.show_about = true,
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_items(nodes: &[Node], out: &mut Vec<Item>) {
        for n in nodes {
            match n {
                Node::Item(i) => out.push(i.clone()),
                Node::Sub { items, .. } => all_items(items, out),
                Node::Separator => {}
            }
        }
    }

    fn items(ed: &Editor) -> Vec<Item> {
        let mut out = Vec::new();
        for m in menus(ed) {
            all_items(&m.items, &mut out);
        }
        out
    }

    #[test]
    fn every_command_in_the_menus_has_a_palette_action() {
        let mut ed = Editor::default();
        for i in items(&ed) {
            if let MenuId::Cmd(c) = i.id {
                // Disabled ones too: the palette must know them all.
                assert!(
                    crate::palette::run_cmd(&mut Editor::default(), c).is_some(),
                    "{c:?} ({}) has no palette action",
                    i.label
                );
            }
        }
        // The menus are the in-window ones, in order.
        let titles: Vec<&str> = menus(&ed).iter().map(|m| m.title).collect();
        assert_eq!(titles, ["File", "Edit", "Pieces", "Mesh", "Tools", "Groups", "Mask", "View", "Plugins", "Help"]);
        ed.view.wireframe = false;
    }

    #[test]
    #[ignore = "timing, run by hand with --ignored --nocapture"]
    fn building_the_menus_is_cheap() {
        let ed = Editor::default();
        let t = std::time::Instant::now();
        let n = 200;
        for _ in 0..n {
            std::hint::black_box(menus(&ed));
        }
        println!("menus(): {:?} each, {} items", t.elapsed() / n, items(&ed).len());
    }

    #[test]
    fn mirror_duplicate_menus_offer_and_run_all_axes() {
        let ctx = egui::Context::default();
        for (title, linked) in [(MIRROR_DUPLICATE, false), (MIRROR_DUPLICATE_LINKED, true)] {
            let mut ed = Editor::default();
            let offset = glam::Vec3::new(2.0, 3.0, 4.0);
            let mut data = oxy_mesh::primitives::icosphere(1, 1.0).to_data();
            data.positions.iter_mut().for_each(|p| *p += offset);
            let mut original = oxy_doc::Piece::new("Ball", oxy_mesh::Mesh::from_data(data));
            original.symmetry_center = Some(glam::Vec3::ZERO);
            let source = original.id;
            ed.replace_document(oxy_doc::Scene::new(original), None);
            let nodes = pieces_menu(&ed, false);
            let axes = nodes
                .iter()
                .find_map(|node| match node {
                    Node::Sub { label, items, .. } if label == title => Some(items),
                    _ => None,
                })
                .unwrap();
            assert_eq!(axes.len(), 3);
            for (node, (label, scale)) in axes.iter().zip([
                ("X", glam::Vec3::new(-1.0, 1.0, 1.0)),
                ("Y", glam::Vec3::new(1.0, -1.0, 1.0)),
                ("Z", glam::Vec3::new(1.0, 1.0, -1.0)),
            ]) {
                let Node::Item(item) = node else { panic!("expected an axis command") };
                assert_eq!(item.label, label);
                ed.scene.set_active(0);
                activate(&mut ed, &ctx, item.id);
                let copy = ed.scene.active();
                assert_eq!(copy.link.map(|link| link.source), linked.then_some(source));
                assert!((copy.mesh.bounds().center() - offset * scale).length() < 1e-5, "{title} {label}");
                assert!(copy.mesh.signed_volume() > 0.0);
            }
            assert_eq!(ed.scene.pieces.len(), 4);
        }
    }

    #[test]
    fn menu_items_change_the_editor_and_show_its_state() {
        let ctx = egui::Context::default();
        let mut ed = Editor::default();
        let find = |ed: &Editor, label: &str| items(ed).into_iter().find(|i| i.label == label).unwrap();

        // A toggle shows its state and flips it.
        let wire = find(&ed, "Wireframe");
        assert_eq!(wire.checked, Some(false));
        assert_eq!(activate(&mut ed, &ctx, wire.id), None);
        assert!(ed.view.wireframe);
        assert_eq!(find(&ed, "Wireframe").checked, Some(true));

        let xray = find(&ed, "Enable X-ray");
        assert_eq!(xray.checked, Some(false));
        activate(&mut ed, &ctx, xray.id);
        activate(&mut ed, &ctx, MenuId::XrayScope(oxy_render::XrayScope::Active));
        assert_eq!(find(&ed, "Enable X-ray").checked, Some(true));
        assert_eq!(ed.view.xray_scope, oxy_render::XrayScope::Active);

        // Undo is off until there's something to undo.
        assert!(!find(&ed, "Undo").enabled);

        // File commands come back for the dialog.
        assert_eq!(
            {
                let id = find(&ed, "Save as…").id;
                activate(&mut ed, &ctx, id)
            },
            Some(UiAction::SaveAs)
        );

        // A list item: the export type.
        let obj = find(&ed, ExportFormat::Obj.label());
        activate(&mut ed, &ctx, obj.id);
        assert_eq!(ed.export_format, ExportFormat::Obj);
        assert_eq!(find(&ed, ExportFormat::Obj.label()).checked, Some(true));

        // A window for the sliders a native menu can't hold.
        {
            let id = find(&ed, "Ambient occlusion, cavity and hard edges…").id;
            activate(&mut ed, &ctx, id)
        };
        assert!(ed.show_shading);

        // A command that changes the model: duplicate the piece.
        let n = ed.scene.pieces.len();
        {
            let id = find(&ed, "Duplicate").id;
            activate(&mut ed, &ctx, id)
        };
        assert_eq!(ed.scene.pieces.len(), n + 1);
        assert!(find(&ed, "Undo").enabled);

        // Boolean lists the other pieces.
        let other = ed.scene.pieces[0].name.clone();
        assert!(
            menus(&ed)
                .iter()
                .flat_map(|m| m.items.iter())
                .any(|n| matches!(n, Node::Sub { label, items, .. } if label == "Boolean"
                && items.iter().any(|s| matches!(s, Node::Sub { label, .. } if *label == format!("With {other}"))))),
            "Boolean offers the other piece"
        );
    }
}
