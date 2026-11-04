//! Built-in guide: short, searchable help topics that work offline.
//!
//! Help > Guide (F2) opens it; the command palette also lists every topic.
//! Keep topics short and practical, and update them when behavior changes
//! (the same rule as the README and the shortcut list).

use crate::editor::Editor;
use egui::RichText;

pub struct Topic {
    pub title: &'static str,
    /// Extra search words that don't appear in the text.
    pub keywords: &'static str,
    /// Paragraphs separated by blank lines; lines starting with "- " are bullets.
    pub body: &'static str,
}

pub const TOPICS: &[Topic] = &[
    Topic {
        title: "Getting around",
        keywords: "camera navigate orbit rotate pan zoom view",
        body: "Drag on empty space to orbit around the model. Right-drag (or Alt+drag on empty space) pans, and the mouse wheel zooms. With a pen, use the drag buttons in the strip beside the viewport.\n\n\
- F frames the whole scene.\n\
- Numpad 1, 3 and 7 snap to the front, right and top views; click the axis gizmo at the top right for the others.\n\
- Hold Shift while orbiting to snap the view to 45° steps.\n\
- O switches between perspective and orthographic.\n\
- Tab hides the panels to give the model the whole window.\n\n\
Prefer starting strokes beside the model? Turn on Settings > Strokes can start on empty space and orbit with the middle mouse button instead.",
    },
    Topic {
        title: "Sculpting basics",
        keywords: "start first stroke clay brush size strength radius",
        body: "Drag on the model to sculpt with the current brush. Raise adds material and Carve removes it; hold Alt to flip for one stroke.\n\n\
- [ and ] make the brush smaller and larger; - and = change its strength. The size/strength bar does the same with the mouse, and so does the quick menu: right-click the model or hold Q.\n\
- Hold Shift to smooth with any brush, Ctrl to mask.\n\
- With a pen, pressure sets the strength (and the radius, if you turn that on in the brush settings).\n\
- Ctrl+Z undoes a stroke, Ctrl+Shift+Z redoes it.",
    },
    Topic {
        title: "Choosing brushes",
        keywords: "brush picker shelf number keys space find",
        body: "Click a brush in the shelf, press its number key (1–9 and 0 select the first ten), or press Space and type part of its name: \"cs\" finds Clay Strips, Enter picks it.\n\n\
Drag brushes in the shelf to put the ones you use most first; the number keys follow the new order. Hover a brush for a one-line description.\n\n\
- Grab, Topological Move and Snake Hook: hold Alt to move the surface along its normal.\n\
- Grab through (Grab settings) moves everything under the brush circle, front to back.\n\
- Middle-drag over a panel scrolls it, handy with a pen side button.",
    },
    Topic {
        title: "Command palette",
        keywords: "ctrl k search commands find action run",
        body: "Press Ctrl+K and type what you want: a command (\"remesh\", \"save as\", \"top view\"), a brush, a recent file or a guide topic. Letters only have to appear in order, so \"adl\" finds Add subdivision level. Up/Down choose, Enter runs, Esc closes.\n\n\
Everything in the palette is also in the menus; the palette just saves you looking for it.",
    },
    Topic {
        title: "Strokes, stabilizer and alphas",
        keywords: "stroke mode freehand line stamp spray lazy alpha texture pattern",
        body: "Stroke mode (in the Stroke section) sets how a drag turns into dabs:\n\n\
- Freehand follows the pointer.\n\
- Line draws a straight line from where you press to where you release.\n\
- Stamp places a single dab: drag out from where you press to size it (good with alphas).\n\n\
The Spray slider scatters dabs around the stroke. The Stabilizer slider makes the brush trail behind the pointer for smooth, controlled lines. Alphas shape each dab: pick one in the Alpha section, or load your own grayscale PNG.",
    },
    Topic {
        title: "Symmetry",
        keywords: "mirror x y z radial",
        body: "X symmetry is on by default: strokes on one side are mirrored to the other. Toggle it with X or in the toolbar; Y and Z are in the Symmetry section, together with radial symmetry (repeat a stroke around an axis, for wheels, flowers or ornaments).",
    },
    Topic {
        title: "Masking",
        keywords: "mask protect freeze unmask invert blur selection",
        body: "Masked areas (shown darker or tinted) are protected from brushes. Hold Ctrl and paint to mask; add Alt to unmask.\n\n\
- Ctrl+drag on empty space masks a rectangle; press L to switch to a lasso.\n\
- Ctrl+click on empty space blurs the mask edge.\n\
- Ctrl+Shift+I inverts the mask, Alt+M clears it, M shows or hides it.\n\
- The Mask menu can also grow, shrink and sharpen the mask, or mask cavities or peaks automatically.",
    },
    Topic {
        title: "Face groups and hiding",
        keywords: "polygroups groups visibility hide show isolate",
        body: "Face groups split a piece into named regions, shown as colors when G is on. Make them from what's visible (Ctrl+W), from the mask, or from loose parts (Groups menu).\n\n\
- Ctrl+Shift+click a group to show only it; Ctrl+Shift+click empty space to show everything.\n\
- Ctrl+Shift+drag shows only what's inside the selection; add Alt to hide it instead.\n\
- Hidden faces are not sculpted or exported. Mesh > Delete hidden removes them for good.",
    },
    Topic {
        title: "Pieces",
        keywords: "objects parts scene add duplicate merge solo",
        body: "A scene can hold several pieces (separate meshes), listed in the Pieces section. Sculpting affects the active piece; Alt+click another piece to make it active.\n\n\
- + Add creates a sphere, cube or plane; Duplicate and Mirror duplicate copy the active piece.\n\
- The eye button hides a piece; Alt+S shows only the active one.\n\
- Merge fuses the visible pieces into one with a voxel remesh.",
    },
    Topic {
        title: "Painting color",
        keywords: "vertex color paint fill cavity dirt",
        body: "The Paint brush (last in the shelf) colors the surface without changing its shape; Alt erases back to white. Pick a color or swatch in its settings. Alphas, spray and symmetry work as with other brushes.\n\n\
- Fill paints everything that isn't masked; Fill cavities and Fill peaks paint creases or edges, more where they are deeper or sharper.\n\
- Clear color returns to unpainted. Use the Pearl or Grey clay material to see true colors.\n\
- Color is saved in projects and follows subdivision levels; remesh and decimate remove it.",
    },
    Topic {
        title: "Posing and the transform gizmo",
        keywords: "gizmo move rotate scale pose transform mask",
        body: "Press T to show the gizmo (T again cycles move, rotate, scale, off). Drag an axis handle, or the centre handle to move in the screen plane, spin around the view or scale evenly. Esc during a drag cancels it.\n\n\
Masked parts stay put and partly masked parts move partly, so a soft mask edge bends smoothly: mask the body, leave an arm unmasked, and rotate it at the shoulder. The pivot sits on the edge of the mask.",
    },
    Topic {
        title: "Subdivision levels",
        keywords: "multires multiresolution detail divide levels",
        body: "Ctrl+D adds a subdivision level: four times the triangles, for finer detail. D and Shift+D move up and down between levels. Make broad changes on a low level and fine detail on a high one; the detail follows when you go back up.\n\n\
Remesh and decimate rebuild the mesh, so they remove the levels (undo brings them back).",
    },
    Topic {
        title: "Remesh and decimate",
        keywords: "topology voxel rebuild reduce polygons triangles resolution",
        body: "Voxel remesh (Ctrl+R) rebuilds the active piece with even triangles, so stretched areas sculpt well again. Resolution sets how fine the new mesh is.\n\n\
Decimate reduces the triangle count to the Keep percentage while preserving the shape, for export or to lighten a heavy piece. Both run in the background and can be undone.",
    },
    Topic {
        title: "Saving, autosave and recovery",
        keywords: "save project oxb autosave crash recover recent files open",
        body: "Ctrl+S saves the whole scene as an .oxb project (pieces, levels, masks and groups). Saves are atomic: a crash while saving never damages the previous file.\n\n\
OxyBrush also autosaves every couple of minutes (Settings > Autosave). If it ever closes unexpectedly, the next start offers to recover your work. File > Open recent and the welcome window list the files you used last.",
    },
    Topic {
        title: "Importing and exporting",
        keywords: "export import obj stl gltf glb ply 3d print file other programs game engine lod split cli",
        body: "Ctrl+I imports an OBJ or STL file; each object in an OBJ file becomes its own piece. Ctrl+E exports every visible piece (hidden faces are left out): glTF/GLB keeps each piece as its own object for game engines and other 3D tools, OBJ writes one object per piece, and STL and PLY combine them (STL is the usual choice for 3D printing).\n\n\
- Pieces > Make LOD chain adds lighter copies at 1/2, 1/4 and 1/8 of the triangles.\n\
- Pieces > Split by face groups or Split into loose parts turns one piece into several.\n\
- The oxybrush-cli command-line tool converts, remeshes, decimates, makes LODs and splits files without opening the app.",
    },
    Topic {
        title: "Layouts and panels",
        keywords: "interface layout touch compact tablet arrange panels scale theme",
        body: "Settings > Layout switches between Standard, Compact (small screens) and Touch friendly (bigger controls and on-screen Smooth/Mask/Invert buttons for tablets). Settings > Arrange panels moves the brush shelf, the settings panel and the size/strength bar.\n\n\
Interface scale and a high-contrast theme are in Settings too. All of these are remembered.",
    },
    Topic {
        title: "Shading and materials",
        keywords: "material matcap ambient occlusion cavity view look flat faceted custom image background gradient color backdrop",
        body: "View > Material picks the look of the clay. Ambient occlusion darkens folds and the inside of shapes, and cavity shading darkens creases and pores, so detail reads clearly; set both in View > Shading, or toggle them with the ball button in the view strip.\n\n\
- Load matcap… (in the material picker or the View menu) uses your own PNG or JPEG image of a lit sphere; it is kept for next time. Right-click it in the picker to remove it.\n\
- View > Flat shading shows every polygon as a flat facet.\n\
- View > Background (also under Settings) switches between a soft gradient and one flat color. Colors… sets the center and the edge, or the single flat color. Reset to default puts the original gradient back. The command palette can open the colors, switch the style or reset it. The choice is remembered.",
    },
    Topic {
        title: "Updates and privacy",
        keywords: "update version network privacy telemetry",
        body: "OxyBrush never contacts the internet unless you turn on update checks in Settings > Updates (or use Help > Check for updates). Then it asks GitHub for the latest release and sends nothing about you or your work. This guide is built in and works offline.",
    },
    Topic {
        title: "Reporting problems",
        keywords: "bug crash feedback log help issue",
        body: "Help > Report a bug opens a pre-filled form in your browser; Help > Copy system info copies the version, system and graphics card for the report. Help > Open log folder shows the log, which helps with crashes.",
    },
];

/// Guide window state.
#[derive(Debug, Default, Clone)]
pub struct GuideState {
    pub open: bool,
    pub query: String,
    pub topic: usize,
}

/// Topics matching every word of `query` (in the title, keywords or text),
/// title matches first.
pub fn search(query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return (0..TOPICS.len()).collect();
    }
    let mut hits: Vec<(bool, usize)> = TOPICS
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let title = t.title.to_lowercase();
            let all = format!("{title} {} {}", t.keywords, t.body.to_lowercase());
            words
                .iter()
                .all(|w| all.contains(w.as_str()))
                .then(|| (!words.iter().any(|w| title.contains(w.as_str())), i))
        })
        .collect();
    hits.sort();
    hits.into_iter().map(|(_, i)| i).collect()
}

pub fn window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.guide.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Guide").open(&mut open).default_size(egui::vec2(640.0, 440.0)).collapsible(false).show(
        ctx,
        |ui| {
            let g = &mut ed.guide;
            ui.add(
                egui::TextEdit::singleline(&mut g.query).hint_text("Search the guide…").desired_width(f32::INFINITY),
            );
            let hits = search(&g.query);
            if !hits.is_empty() && !hits.contains(&g.topic) {
                g.topic = hits[0];
            }
            ui.add_space(4.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    egui::ScrollArea::vertical().id_salt("guide_topics").show(ui, |ui| {
                        if hits.is_empty() {
                            ui.label(RichText::new("No topic matches").weak());
                        }
                        for &i in &hits {
                            ui.selectable_value(&mut g.topic, i, TOPICS[i].title);
                        }
                    });
                });
                ui.separator();
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical().id_salt("guide_body").show(ui, |ui| {
                        if let Some(t) = hits.contains(&g.topic).then(|| &TOPICS[g.topic]) {
                            ui.label(RichText::new(t.title).strong().size(16.0));
                            ui.add_space(4.0);
                            for para in t.body.split("\n\n") {
                                for line in para.lines() {
                                    match line.strip_prefix("- ") {
                                        Some(b) => {
                                            ui.horizontal_wrapped(|ui| {
                                                ui.label("•");
                                                ui.label(b);
                                            });
                                        }
                                        None => {
                                            ui.label(line);
                                        }
                                    }
                                }
                                ui.add_space(6.0);
                            }
                        }
                    });
                });
            });
        },
    );
    if !open {
        ed.guide.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_all_words_and_ranks_titles_first() {
        assert_eq!(search("").len(), TOPICS.len());
        let mask = search("mask");
        assert_eq!(TOPICS[mask[0]].title, "Masking");
        assert!(mask.len() > 1, "other topics mention masks too");
        let lasso = search("lasso");
        assert_eq!(lasso.iter().map(|&i| TOPICS[i].title).collect::<Vec<_>>(), ["Masking"]);
        assert!(search("stl export").iter().any(|&i| TOPICS[i].title == "Importing and exporting"));
        assert!(search("zzzz nothing").is_empty());
    }

    #[test]
    fn topics_are_well_formed() {
        for t in TOPICS {
            assert!(!t.title.is_empty() && t.body.len() > 40, "{}", t.title);
            assert!(t.keywords.chars().all(|c| c.is_lowercase() || c == ' ' || c.is_ascii_digit()), "{}", t.title);
        }
    }
}
