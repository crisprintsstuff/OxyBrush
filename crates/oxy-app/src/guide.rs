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
        title: "Sculpting a character from OxyCharacter",
        keywords: "oxycharacter character link linked send back return morph accessory oxylink sidecar",
        body: "When OxyCharacter opens a character in OxyBrush, the top bar shows Linked to OxyCharacter.\n\n\
- Sculpt the body, and add pieces for clothes or props.\n\
- Click Linked to OxyCharacter (or File > Send back to OxyCharacter, or Ctrl+K) to save to the file OxyCharacter is watching. It picks up the changes: sculpting on OxyCharacter Body becomes a morph, new pieces become accessories. This works even after a Save As elsewhere; Ctrl+S saves as usual.\n\
- To keep the body a morph, add detail with subdivision levels (Ctrl+D). Remesh, Subdivide once, Decimate, Boolean, Split and Delete hidden change its topology, and it then returns as a separate mesh; OxyBrush tells you the first time.\n\
- The .oxylink.json file next to the project is what links it. Save As copies it next to the new file; OxyBrush never changes or deletes it.",
    },
    Topic {
        title: "Getting around",
        keywords: "camera navigate orbit rotate pan zoom view focal length fov lens",
        body: "Drag on empty space to orbit around the model. Right-drag (or Alt+drag on empty space) pans, and the mouse wheel zooms. With a pen, use the drag buttons in the strip beside the viewport.\n\n\
- F centres and frames the active piece; Alt+F frames every visible piece.\n\
- Numpad 1, 3 and 7 snap to the front, right and top views; click the axis gizmo at the top right for the others, or drag on it to orbit freely.\n\
- Hold Shift while orbiting to snap the view to 45° steps.\n\
- O switches between perspective and orthographic (P in the Single-key tools scheme).\n\
- The lens button in that strip, or View > Focal length, sets how much perspective you see. See Focal length.\n\
- Tab hides the panels to give the model the whole window. Shift+Tab, or the top button of the view strip, hides just the settings panel.\n\n\
Prefer starting strokes beside the model? Turn on Settings > Strokes can start on empty space and orbit with the middle mouse button instead.\n\n\
Every shortcut and the mouse buttons can be changed in Settings > Keymap, including switching between the Default and Single-key tools shortcut schemes. The Right-drag orbit preset orbits with the right button; the MMB orbit preset uses the middle button to orbit, Ctrl+middle to zoom and Shift+middle to pan.",
    },
    Topic {
        title: "Sculpting basics",
        keywords: "start first stroke clay brush size strength radius orbit pivot rotate around gravity sag drape droop hang",
        body: "Drag on the model to sculpt with the current brush. Raise adds material and Carve removes it; hold Alt to flip for one stroke.\n\n\
- [ and ] make the brush smaller and larger; - and = change its strength. The size/strength bar does the same with the mouse, and so does the quick menu: right-click the model or hold Q.\n\
- The brush size is on screen: it stays the same size as you zoom, so zooming in gives finer strokes. Brush settings > Size > On the model makes it a size on the model instead (in scene units, for all brushes): zoom in and the circle grows with the model, so a stroke covers the same area at any zoom.\n\
- S, U and I open a slider right under the pointer for size, strength and Paint strength; Shift+O opens one for focal shift (O in the Single-key tools scheme). Press the key again, Esc, or click outside to close it.\n\
- Hold Shift to smooth with any brush, Ctrl to mask.\n\
- Gravity (in the brush settings) makes the surface sag downward as you sculpt, for drapery and hanging folds. The slider sets how strong (at 1 a pass sags about a brush radius; 0.1–0.3 for a gentle droop). Each brush keeps its own Gravity; it pulls toward the floor (world down), not the bottom of the screen.\n\
- With a pen, pressure sets the strength (and the radius, if you turn that on in the brush settings).\n\
- Orbiting turns around the point of the model under the pointer where you start it (from empty space, the nearest part of the model), and that point stays put on screen, so you can turn around any spot you point at. Preferences > Orbit around can make it the last stroke instead, or the centre of the view. F frames the model and goes back to orbiting around the middle until you orbit or sculpt again.\n\
- Ctrl+Z undoes a stroke, Ctrl+Shift+Z redoes it. Ctrl+Alt+Z undoes on the active piece only, skipping changes to other pieces (Preferences > General can make that the default).",
    },
    Topic {
        title: "Pen tablets and stylus",
        keywords: "tablet pen stylus side button barrel windows ink pressure curve sensitivity firm soft wacom huion kamvas xppen x11 wayland linux cachy scroll panel hold gesture",
        body: "OxyBrush supports pressure-sensitive stylus and drawing tablets (Wacom, Huion, XP-Pen, etc.).\n\n\
- Stylus pressure controls brush strength, and can also modulate brush radius when enabled in the brush settings.\n\
- Preferences > Pen & tablet sets how pressure feels: drag the two points on the curve (an S shape gives fine control at both ends; double-click to straighten), the curve bend, Lightest touch (the strength of a very light stroke) and Full pressure at (how hard you press for full strength), with Soft, Linear and Firm presets. While you draw, a dot shows your pen on the curve. If your tablet driver has its own pressure curve, keep that one straight so the two don't add up.\n\
- There are two curves: Strength, for brushes with Pen pressure sets strength, and Size, for brushes with Pen pressure sets radius, so a light touch can give a thin line at full strength, or a wide soft one. Curve, Lightest touch and Full pressure at apply to both.\n\
- In panels and lists, drag with the pen to scroll (or drag with a side button held), and hold the pen still on a control for a moment to open its menu, like a right-click, and read its tip. Preferences > Pen & tablet sets when dragging scrolls: with a pen, always (for pens whose drivers send no pressure) or never.\n\
- In the viewport, the Pen side buttons navigation preset (Preferences > Keymap, or the button in Pen & tablet) makes the lower side button orbit, with Shift pan and with Ctrl zoom, and the upper side button pan.\n\
- Pen side buttons can be mapped to right-click (quick menu), middle-click (navigation/scrolling), or modifier keys in your tablet driver.\n\
- On Windows with Windows Ink, the pen's side button already acts as the right mouse button: a tap opens the quick menu, and with the Right-drag orbit preset (Settings > Keymap) holding it while you drag orbits the view and Ctrl + side button zooms.\n\
- On Linux under Wayland compositors (CachyOS, KDE Wayland, GNOME Wayland, Hyprland), drawing tablets require the X11/XWayland backend because native Wayland tablet events are not exposed to winit. OxyBrush automatically detects connected tablets and defaults to the X11 backend when available. You can also manually force X11 mode by starting with `oxybrush --x11` (or `oxybrush --backend x11`), or by setting `OXY_BACKEND=x11`.",
    },
    Topic {
        title: "Brush cursor",
        keywords: "ring circle orientation surface normal tangent align screen",
        body: "The ring under the pointer is the brush cursor. It shows the brush size, and the inner ring is where the falloff is about halfway. Focal shift (in the toolbar Size/Strength bar, the brush settings, the quick menu, or Shift+O) moves the inner ring out for a firmer brush and in for a softer one.\n\n\
By default the ring lies flat on the screen, so it stays a circle at any viewing angle. View > Align cursor to surface (the same checkbox is in Settings, and the command palette finds it too) lays the ring on the clay instead: a circle the size of the brush, flat against the surface, with a short line showing which way the surface faces.\n\n\
That direction is an average of the surface around the pointer, eased as you move, so on a dense mesh it stays steady instead of twitching from one triangle to the next. Move the pointer off the model and the ring goes back to lying flat on the screen. The choice is remembered.",
    },
    Topic {
        title: "Choosing brushes",
        keywords: "brush picker shelf number keys space find hotkey shortcut",
        body: "Click a brush in the shelf, press its number key (1–9 and 0 select the first ten), or press Space and type part of its name: \"cs\" finds Clay Strips, Enter picks it.\n\n\
Each brush keeps its own settings: falloff, focal shift, spacing, stroke mode, stabilizer, auto smooth, spray, the pressure switches and its alpha come back when you pick it again (strength and gravity too). Size stays one for all brushes unless Preferences > General > Size per brush is on; Settings per brush (same place) turns the whole thing off. Remember brush settings between sessions keeps them all for next time.\n\n\
Drag brushes in the shelf to put the ones you use most first; the number keys follow the new order. To give a brush a key of its own, right-click it in the shelf and choose Set hotkey, then press the key (with Ctrl, Shift or Alt if you like); Remove hotkey takes it away, and Preferences > Keymap lists them under Brush keys. Hover any control for a short explanation and its shortcut. On a touch screen, press and hold the control: the same text stays up after you lift your finger, and the control does not activate.\n\n\
- Grab, Topological Move and Snake Hook: hold Alt to move the surface along its normal.\n\
- Kelvin Grab pulls the surrounding surface elastically. Drag sideways with Kelvin Twist to rotate around the surface normal. Drag up or down with Kelvin Scale to expand or contract. Alt pulls Grab along the normal and reverses Twist and Scale.\n\
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
        keywords: "stroke mode freehand line stamp curve spline editable adjust hold fade taper spray lazy alpha texture pattern",
        body: "Stroke mode (in the Stroke section) sets how a drag turns into dabs:\n\n\
- Freehand follows the pointer.\n\
- Line draws a straight line from where you press to where you release.\n\
- Stamp places a single dab: drag out from where you press to size it (good with alphas).\n\
- Curve draws a stroke that stays editable: drag its points to reshape it, click the curve to add a point, Alt+click a point to remove it. Drag the window by its title bar to move it. Size, strength and the Fade in / Fade out sliders update it live. Accept (Enter) applies it as one undo step, as does starting the next stroke; Cancel (Esc) discards it. You can orbit around it while it is open.\n\
- In Freehand mode, hold the pen or mouse still for about half a second while drawing: a ring fills up at the cursor and the stroke becomes a curve; keep moving to extend it. Turn this off with Hold to adjust in the Stroke section, or tick Ask first to be asked each time.\n\
- Edit > Last stroke can Replay the Freehand stroke you just made (stamp it again without undoing), or reopen it: Edit as curve (same brush, editable, Enter applies it), Use as Curve Move curve, or Use as tube path. Replay stays available until remesh or a new freehand stroke; Edit as curve works right after the stroke, even if you've turned the view since.\n\
- While a curve is open, click a point to give it its own size and strength (a ring shows the size), tick Join the ends for a loop, and use Ctrl+Z / Ctrl+Shift+Z to undo and redo your curve edits.\n\n\
The Spray slider scatters dabs around the stroke. The Stabilizer slider makes the brush trail behind the pointer for smooth, controlled lines. Alphas shape each dab: pick one in the Alpha section (built-ins include overlapping Scales, Snake scales, Diamond scales, Scale plate for Stamp, and Scale strip for Roll), or load your own PNG, JPEG, TIFF, WebP, or an 8- or 16-bit grayscale or RGB PSD with a saved composite.\n\
- Tile wraps a seamless pattern inside each dab; Repeats sets how many periods fit across the brush.\n\
- Roll scrolls the alpha along the stroke like a paint roller, best with a tileable strip such as Scale strip. Roll distance sets the length of each repeat. Type a value and press Enter to set it exactly. Follow stroke direction stays on while rolling.\n\
- Rotation turns the alpha and updates its thumbnail as you adjust it. Rotate jitter turns the alpha randomly each stroke or each dab.\n\nCapture from view (in the Alpha section, or the command palette) makes an alpha from what you see: a depth image of the model in the middle of the view, nearest parts white, cropped to the model, at the size picked next to the button (up to 4096 px; bigger gives smoother edges). Sculpt or arrange a detail (a rivet, a scale, a button), look at it straight on, capture it, then stamp it anywhere with Stamp strokes. Captured and loaded alphas are saved with the project, and come back when you open it. Export alpha… saves the selected alpha as a 16-bit grayscale PNG, to use in other projects or edit in a 2D app and load it back with Load image….",
    },
    Topic {
        title: "Symmetry",
        keywords: "mirror x y z radial",
        body: "X symmetry is on by default: strokes on one side are mirrored to the other, and so are mask boxes and lassos. Toggle it with X or in the toolbar; Y and Z are in the Symmetry section, together with radial symmetry (repeat a stroke around an axis, for wheels, flowers or ornaments).\n\nEach piece mirrors about its own centre, chosen from its shape when it's imported or opened; a model built around the origin keeps the origin. If mirroring lands in the wrong place (say, after moving the whole piece), click Recentre in the Symmetry section. Turning a mirror on briefly outlines its plane.\n\nSymmetry mirrors new strokes; it doesn't fix a model whose sides already differ. For that, Tools > Symmetrize keeps one half (for example +X onto −X: the +X side is where the axis gizmo's red X points), replaces the other half with its mirror image and welds the two along the middle, about the piece's centre. It's one undo step and removes subdivision levels.",
    },
    Topic {
        title: "Living strokes",
        keywords: "strokes edit past fade delete solo layer layers group groups mask unmasked strength history bake save mirror copy split side shape keys morph targets export gltf",
        body: "Living strokes keeps each stroke editable after you make it. It's off by default: turn it on in Preferences > General, or with Turn on Living strokes in the right panel's Layers section. Strokes made before turning it on aren't listed.\n\
- The Layers section in the right panel is where you work with layers while you sculpt: Sculpt into and + New layer, then each layer with its eye, strength slider and … menu. Its Strokes… button opens the full list of strokes, for work on single strokes.\n\n\
- The Strokes window lists the active piece's strokes, newest first. Click one to select it; Shift or Ctrl+click selects several. Double-click a stroke, or select one and press Edit…, to change its brush, Raise or Carve, size, strength, falloff, alpha or spacing. The stroke is sculpted again as you change it; strokes made after it stay as they were. Each change is an undo step, and Esc puts the stroke back as it was when you opened Edit. A stroke that can't be edited says why (made before this version, dynamic detail, a mirrored copy, or a whole-piece tool).\n\
- Strength fades the selected strokes from 0% (gone) to 150% (exaggerated), live while you drag. That fade is separate from the brush strength in Edit.\n\
- Delete takes their change off the surface; Solo shows them without the strokes made after them until you click Show all.\n\
- Keep only where unmasked takes part of a stroke away: mask the part you don't want, select the stroke, press it, then clear the mask.\n\
- Group puts the selected strokes in a named group, used like a sculpt layer: its box shows or hides them all, its slider sets their strength together. Click a group's name to select its strokes, double-click to rename it; its … menu merges it into another group, ungroups it (the strokes keep how they look) or deletes its strokes. Add to group moves strokes into a group, where they take its strength.\n\
- Sculpt into (at the top) picks a layer for new strokes before you sculpt: + New layer starts one, named when your first stroke makes it. New strokes go into it and follow its strength and eye, so on a layer at 50% a stroke settles to half when you let go. Strokes don't go into a hidden layer.\n\
- Mirror copy puts the selected strokes on the other side of the piece's symmetry plane (X, or Y or Z from the box next to it), as new strokes you can fade on their own. Each point lands on the point nearest its mirror position, so it works where the piece is symmetric.\n\
- Split by side turns a stroke made with symmetry on into one stroke per side, so you can fade or delete just the left wrinkle. Nothing on the model moves.\n\
- Exporting to glTF or GLB writes each group as a shape key (morph target) named after it, on the piece without the groups: in Blender or a game engine the groups are sliders. Hidden groups are in the file at 0. Other formats get the surface as you see it.\n\
- Later strokes keep what they did: a stroke's change is added or taken away, not replayed, so a stroke smoothed over later can look different when faded.\n\
- Every change is an undo step. Undoing a stroke hides it from the list, and redo brings it back.\n\
- Adding or switching subdivision levels keeps sculpt and paint layers editable. A remesh, dynamic detail, or another operation that replaces the topology bakes the strokes into the surface. Bake all does that by hand. The oldest strokes are baked when the list passes its memory budget (Preferences).\n\
- Strokes and groups are saved with the project, so they stay editable after you reopen it (turn off Preferences > General > Save strokes with the project for smaller files). Versions before 4.0.0-beta.9 open such files with the strokes fixed in place.\n\
- With Living strokes on, a masked gizmo pose can be saved to a layer so you can fade it afterward. With Also paint on, RGB Strength controls the paint independently of the sculpt strength.\n\
- Mask strokes aren't listed.",
    },
    Topic {
        title: "Recipe Box",
        keywords: "recipe recipes box kitbash detail details reuse place drop move turn scale mirror pair linked unlink save back new library",
        body: "The Recipe Box is a library of reusable sculpted details: an eye, a scar, a row of scales. Open it from View > Recipe Box (a movable window; close with ×, and the choice is remembered). Living strokes must be on.\n\n\
- To make a recipe, sculpt the detail, select its strokes in the Strokes window and choose Save as recipe….\n\
- Drag a recipe onto a model to sculpt it there: Place one, or Mirror pair for both sides. It's sculpted again on that surface, so any model works.\n\
- Click a recipe slot to rename it, change its category, move it earlier or later, or delete it from the box. Recipes in the same category appear under Variations. Click a variation to place it or drag it in the row to reorder it.\n\
- A placed recipe is a group in the Strokes window, and its strokes stay editable (Edit…, Edit path). Its … menu has the rest.\n\
- Move on surface: click on the model where it should go; the Move window sets Twist and Size, Apply keeps it in place with them, and Cancel or Esc leaves it. One undo step. Strokes you made after it stay as they were.\n\
- A mirrored pair is Linked: Edit…, Edit path and Move on surface change both sides; fade and delete change one side. Unlink stops it, and undo links them again.\n\
- Save back to Recipe Box replaces the recipe in the box with the version on the model (it asks first; fades aren't saved). Save as new recipe adds it under a new name. Edits to a placed recipe reach the library only when you choose one of these.",
    },
    Topic {
        title: "Deform window",
        keywords: "deform bend twist taper skew flatten squeeze sag inflate spherize noise polish relax smooth unify",
        body: "Tools > Deform… changes the whole piece at once: bend, twist, taper, skew, flatten, squeeze, sag, inflate, spherize, noise, smooth, polish, relax and more.\n\n\
- Drag a bar left or right to see the result on the model; let go to apply it as one undo step. The bar springs back, so another drag adds more. Escape while dragging cancels.\n\
- X, Y and Z next to each bar pick the axes it uses.\n\
- Masked parts stay put. Turn off Use mask to deform the whole visible piece.\n\
- Repeat on this piece applies the last one again; Repeat on other pieces applies it to every other visible piece.\n\
- Fit to standard size centres the piece and scales it so its longest side is 2 units.",
    },
    Topic {
        title: "Masking",
        keywords: "mask protect freeze unmask invert blur selection",
        body: "Masked areas (shown darker or tinted) are protected from brushes. Hold Ctrl and paint to mask; add Alt to unmask.\n\n\
- Ctrl+drag on empty space masks a rectangle; press L (or use Selection in the Mask panel) to switch to a lasso, a circle, an ellipse or a curve. With symmetry on, the mirrored area is masked too.\n\
- Circle: drag out from its centre. Ellipse: drag from one corner of the box it fills to the other. With either, Ctrl+drag draws it wherever it starts, on the model too.\n\
- Curve: Ctrl+drag from the first point (Ctrl+Shift for visibility) and let go to apply, or Ctrl+click it, click the others and press Enter (or click the last point again). The line runs straight from point to point; tap Alt while drawing to add a point it bends smoothly through, and tap again quickly to make that point a sharp corner. Everything on one side of the line is masked (or shown): the side on your right as you draw, so below a line drawn left to right; hold Alt when you apply for the other side. Ticks on the line show the side. Its ends carry on straight to the edge of the view. Backspace takes the last point back, Esc cancels.\n\
- Ctrl+click on empty space inverts the mask (with no mask, that masks everything). A Ctrl box around empty space clears it.\n\
- Ctrl+Shift+I inverts the mask, Alt+M clears it, M shows or hides it. Mask > Fill masks everything. Invert and Fill work on what's visible: hidden parts keep their mask.\n\
- The Mask menu can also grow, shrink and sharpen the mask, or mask cavities or peaks automatically. Mask border masks the edge of an open surface or of what's left visible after hiding part of it; Mask group borders masks the lines where face groups meet. Grow the mask to widen either.\n\
- Edge loop at mask border (Mask and Mesh menus) adds a loop of quads along the edge of the mask, inside it or on both sides (Both sides), at the Width you set: support loops, so a hard edge (a panel, the top of a box) stays sharp when you subdivide or smooth. A triangle counts as masked when all its corners are, so masking a face group (Ctrl+M) puts the loop exactly around it. The loop gets its own face group.\n\
- Subdivide masked (Mask and Mesh menus) adds resolution only where you mask: the masked triangles split into four, their neighbours just enough to keep the surface closed. Ctrl+M on a face group (or Groups > Group by islands first, for an imported model whose parts are one piece) masks one part, like a single feather. Smooth (Topology > Subdivision levels) curves the new points with the surface; existing points never move. Repeat it for finer still.\n\
- Split masked moves the masked part into a new piece (Invert first to split off the unmasked part). Extract mask… makes a new shell from the masked area and leaves the piece whole.",
    },
    Topic {
        title: "Face groups and hiding",
        keywords: "polygroups groups visibility hide show isolate paint groups brush",
        body: "Face groups split a piece into named regions, shown as colors when G is on. Make them from the mask (Ctrl+Shift+W; with nothing masked it groups what's visible), from what's visible, or from loose parts (Groups menu).\n\n\
- The Paint Groups brush (in the brush shelf, after Paint) paints them directly: each stroke makes a new group, and Alt+stroke grows the group you start on. Shift+stroke smooths group borders: each face joins the group most of its neighbours are in, so spikes and single stray faces go and straight borders stay. Symmetry and Front faces only apply, hidden faces are left alone, the surface never moves, and each stroke is one undo step. It turns the group colors on while you paint.\n\
- Ctrl+Shift+click a group to show only it, Ctrl+Shift+Alt+click a group to hide it; Ctrl+Shift+click empty space to show everything.\n\
- Ctrl+Shift+drag shows only what's inside the selection; add Alt to hide it instead. Drawn around none of the model (on empty space), it inverts visibility.\n\
- Hold Space while drawing a box or lasso (for masking or visibility) to move it: it keeps its shape and follows the pointer.\n\
- Hidden faces are not sculpted or exported. Mesh > Delete hidden removes them for good, and Mesh > Split hidden moves them into a piece of their own.\n\
- Mesh > Close holes fills every opening in the piece (after deleting faces, a cut, or an import with gaps) with a smooth patch in a new face group, so the piece is closed again, as 3D printing needs.",
    },
    Topic {
        title: "Pieces",
        keywords: "objects parts scene add duplicate merge join combine split detach solo linked copy instance clone mirror unlink boolean seam blend",
        body: "A scene can hold several pieces (separate meshes), listed in the Pieces section. Sculpting affects the active piece; Alt+click another piece to make it active.\n\n\
- + Add opens a window for a sphere, cube, plane, cylinder, cone, torus or disc (a flat round plate of quads: an Inner radius above 0 makes a ring, Rings and Segments around set the faces, and Slice keeps only the part between two angles): set its size and segments (a sphere can be Quads, a cube pushed out into a ball with even quads and no poles; UV, rings from pole to pole; or Triangles, like the starting sphere) (low-poly by default, or tick Even triangles to remesh it for sculpting); the live preview shows polygon edges so you can see divisions while adjusting, then Accept; Duplicate and Mirror duplicate copy the active piece, with all its subdivision levels.\n\
- The eye button hides a piece; Alt+S shows only the active one.\n\
- Preferences > General > Lighter inactive pieces (off by default) keeps the view quick in scenes with several detailed pieces: while you sculpt or edit one piece, the others with subdivision levels are drawn at their lowest level, and in full again as soon as you stop. Only the drawing changes: every piece keeps its levels, and saving, export and undo always use them in full.\n\
- Pieces > Duplicate linked and Mirror duplicate linked make a linked copy: it follows the original as you sculpt, paint or remesh that, live. Move, turn or scale the copy with the transform gizmo and it stays linked where you put it; moving the original moves its copies too (a mirrored copy the other way). Copies can't be sculpted themselves; Pieces > Unlink copy makes one an ordinary piece. Saved projects keep the link; older versions open the copy as an ordinary piece.\n\
- Booleans (Subtract, Add, Overlap) work on voxels at the Resolution under Voxel remesh. Seam blend rounds the seam where the pieces meet over that many voxels (2 by default): a sharp seam (0) comes out jagged. The rest of the active piece keeps its shape, so earlier seams and detail stay as they were.\n\
- Join combines the active piece and the other visible pieces into one piece, exactly as they are: nothing is remeshed or welded, and each keeps its own face groups, so you can still hide or select it. Merge + remesh instead fuses them into one closed surface with new, even triangles (a voxel remesh). Merge down joins just the active piece and the one below it in the list, the same way.\n\
- Ctrl/Cmd+click pieces in the list to pick them along with the active one. The transform gizmo (T) then moves, turns and scales them all together about their middle, as one undo step. A plain click on a piece, or One piece in the gizmo bar, goes back to one.\n\
- Mesh > Split masked (also in the Mask menu) moves the masked part of the active piece into a new piece; invert the mask first to split off the unmasked part instead.",
    },
    Topic {
        title: "Surface instances",
        keywords: "instances instancing copies copy clone scatter rivets spikes scales studs array repeat make real source",
        body: "Copy one piece onto the surface of another: a rivet on every face of a plate, spikes on a ball, scales on one face group. The copies follow the surface as you sculpt it.\n\n\
- Make the piece to copy first (add or import it), then make the other piece active. In the right panel's Instances section, pick the piece to copy and click Copy onto this piece. The copied piece is hidden and marked \"copied\" in the Pieces list.\n\
- On: every face (quads stay whole), every point, or only the faces of one face group (Groups menu to make one).\n\
- Facing: which way each copy turns around the surface normal. World direction (+Z by default) keeps neighbours facing the same way; Longest edge and First edge follow each face but can flip between neighbours; Random turns each one differently.\n\
- Size is the copy's largest side, as a share of the face or in scene units. Lift raises copies off the surface or sinks them in; Turn spins them all; Vary size and Vary turn add randomness, and Seed picks another random mix.\n\
- Edit source shows the copied piece alone, with every tool; Done editing source goes back. Every copy follows the changes.\n\
- Make real turns the copies into a new piece of ordinary mesh, one face group per copy, and keeps the copies, so you can change them and make them real again. Remove deletes the copies and shows the copied piece again.\n\
- Every change is one undo step. Exports don't include the copies: Make real first.",
    },
    Topic {
        title: "Scale cover",
        keywords: "scale cover dino mode scales plates armor teardrop diamond spike ridge scatter mask face group instances",
        body: "Scale cover (Tools > Scale cover…; Discord nickname “Dino mode”) places overlapping scale plates on the active piece using surface instances.\n\n\
- Pick a built-in plate (teardrop, diamond, round, plate, ridge, spike) or Use active piece as plate for a custom sculpt.\n\
- Placement: even Scatter across the visible surface, Mask (only masked faces), or one Face group.\n\
- Size, Spacing, Lift, variation and Seed control density and look; Orient defaults to Longest edge so rows share a grain (flow along limbs comes later).\n\
- Apply creates or updates the cover. Edit source sculpts one plate for every copy; Make real bakes ordinary mesh; Remove clears the cover.\n\
- For underlay grit between plates, run Surface Noise on the host first.",
    },
    Topic {
        title: "Painting color",
        keywords: "vertex color colour paint fill cavity dirt filter hue saturation brightness contrast smooth import obj ply mrgb painted show hide toggle view",
        body: "The Paint brush (last in the shelf) colors the surface without changing its shape; Alt erases back to white. Pick a color or swatch in its settings. Alphas, spray and symmetry work as with other brushes.\n\n\
- Fill paints everything that isn't masked; Fill cavities and Fill peaks paint creases or edges, more where they are deeper or sharper.\n\
- Color Filter… (Tools > Vertex color, or the command palette) opens a window of whole-piece adjustments: Fill, Hue, Saturation, Value, Brightness, Contrast, Smooth and Red/Green/Blue. Drag a bar to preview; let go to apply as one undo step. Masked parts stay put unless Use mask is off. Fill uses the current Paint color.\n\
- Clear color returns to unpainted. Use the Pearl or Grey clay material to see true colors.\n\
- Color is saved in projects, follows subdivision levels, and is kept through remesh and decimate.\n\
- Painted models from other apps come in with their colors: import an OBJ (colors after each vertex, or the #MRGB comment blocks some sculpting apps write) or a PLY with vertex colors. Nothing needs turning on; paint over them as usual. OBJ, PLY and glTF exports carry the colors back out.\n\
- View > Shading > Show vertex color (also in the Vertex color menu) hides the colors to see the form in plain clay, and shows them again. They're kept while hidden; painting or filling shows them again.",
    },
    Topic {
        title: "Posing and the transform gizmo",
        keywords: "gizmo move rotate scale pose transform mask symmetry mirror symmetric",
        body: "Press T to show the gizmo and T again to hide it. It moves, turns and scales in one: drag an arrow or a square to move, a ring to turn (the outer ring turns around the view), a cube partway along an axis to scale along it, or the circle round the centre to scale evenly. The gizmo bar can also show Move, Rotate or Scale on its own, for a less crowded gizmo. With a single one, drag an axis handle, or the centre handle to move in the screen plane, spin around the view or scale evenly. Esc during a drag cancels it.\n\n\
Masked parts stay put and partly masked parts move partly, so a soft mask edge bends smoothly: mask the body, leave an arm unmasked, and rotate it at the shoulder. The pivot sits on the edge of the mask, and moves there again after each change; to keep it where it is while you mask and unmask, untick Follow the mask in the gizmo bar's Pivot menu (or in Preferences > General).\n\n\
The fields under the gizmo show how far it has moved, turned or scaled since the gizmo opened, and follow along while you drag. To land on an exact value, type it and press Enter: typing 2 in Scale makes it twice the size it started at. The second row shows the actual Size and Centre of what the gizmo moves, along the world axes (Mesh > 3D Print Tools shows the size in millimetres). Type a size or a centre and press Enter to set it exactly; with Even on, the other sides of the size follow in proportion. Apply keeps the changes and counts from zero again; Cancel puts them back. Done closes the gizmo and keeps your changes. If you'd rather Done put back anything you didn't apply, like Cancel, turn on Preferences > General > Closing the gizmo puts back changes not applied (Redo brings changes back either way). Operators (mirror, put on the ground, centre at origin) are applied at once.\n\nWith Mirror X (or Y, Z) on, the gizmo follows it like the brushes: when what it moves sits on both sides of the mirror plane (a pair of shapes inserted with Mirror X, a symmetric model) and the pivot is on one side, the other side gets the mirrored move, turn or scale. A part that crosses the plane stretches symmetrically. A part off to one side, or a pivot on the plane, moves everything as one.",
    },
    Topic {
        title: "Bending with Curve Move",
        keywords: "curve move bend pose arm tail limb deform stretch wire",
        body: "Curve Move bends and stretches a long part (an arm, a tail, a horn) along a curve. Pick it from the brushes, draw a curve along the part, then drag the curve's points: the part follows, bending smoothly. With Through the middle on, the curve runs through the part's core rather than over its skin.\n\n\
Size sets how far from the curve the model follows: make the circle cover the part's thickness so it moves as a whole. Masked areas never move, and symmetry moves the mirrored side too. Click the curve to add a point, Alt+click to remove one, select a point to change its reach and strength there, and Reset to start over. Drag the Curve Move window by its title bar to move it. Enter applies it as one undo step; Esc cancels.",
    },
    Topic {
        title: "Subdivision levels",
        keywords: "multires multiresolution detail divide levels quads catmull clark loop triangles subdivide uv edge loops",
        body: "Ctrl+D adds a subdivision level: four times the triangles, for finer detail. D and Shift+D move up and down between levels. Make broad changes on a low level and fine detail on a high one; the detail follows when you go back up.\n\n\
Levels are made of quads by default (Catmull–Clark): every polygon becomes quads, so the edge loops of a quad base mesh carry on up and every level exports as quads. A triangle becomes three quads. Choose Triangles (Loop: each triangle becomes four) under Subdivision levels in the Topology panel before adding the first level; a piece keeps the kind its levels were made with. Poly Edit creases stay sharp in the levels. The same choice applies to Mesh > Subdivide.\n\n\
Smooth (on by default) rounds each new level off. Turn it off to only split the faces, with new points at the middle of edges and faces, so the shape stays exactly as it is: good for hard-surface pieces or for more resolution without softening. Each level keeps how it was made.\n\n\
Rebuild lower level in Topology recovers a coarser base from a compatible closed Catmull–Clark quad mesh without UVs or explicit creases. Bake editable strokes first. Repeat it to recover another level when the base still qualifies. The displayed level keeps its detail.\n\
\n\
Remesh and decimate rebuild the mesh, so they remove the levels (undo brings them back).",
    },
    Topic {
        title: "Smooth preview",
        keywords: "smooth preview dynamic subdivision catmull clark cage low poly creases subdivide smooth topology",
        body: "Smooth preview shows what Catmull–Clark subdivision would make of a low-polygon piece, without changing the mesh yet. In Topology, set Preview to 1, 2 or 3 levels (or use Smooth > Preview in Poly Edit). You keep editing the cage; the viewport draws the smooth surface. Creases from Poly Edit stay sharp. Subdivide (smooth) replaces the cage with that surface in one undo step. It needs a low-polygon piece without subdivision levels.",
    },
    Topic {
        title: "Brush favorites and presets",
        keywords: "favorites favourites star shelf preset rename duplicate palette brush editor custom shelves",
        body: "Right-click a brush on the shelf and choose Add to Favorites to pin it on the Favorites strip above the full list. In Presets…, right-click a saved preset or use its row action button to star, rename, duplicate, or share it. Starred presets sort to the top. Ctrl+K or Space finds Preset: entries by name.",
    },
    Topic {
        title: "Dynamic detail",
        keywords: "dynamic topology detail triangles adaptive refine sphere sketch",
        body: "Turn on Dynamic detail in the Topology section (or from the command palette) and sculpting brushes add triangles where they need more detail, and remove ones that have become too small. Start from a plain sphere and pull out shapes with Snake Hook, build up with Clay, without remeshing.\n\n\
How fine it gets depends on the Detail slider and the brush size: a smaller brush gives smaller triangles. Mask, Paint, the Grab-like brushes and the cutting tools don't change triangles. Each stroke is one undo step. It is off on pieces with subdivision levels, since it would remove them: Keep this level only (in the Topology section or the Mesh menu) turns the level you see into the piece's mesh, and then it works. It works best up to a few hundred thousand triangles.",
    },
    Topic {
        title: "Tubes",
        keywords: "tube pipe horn tentacle tail limb cable path curve sweep draw twist spin coil spring spiral helix braid plait rope strands weave",
        body: "The Tube brush (in the brush shelf, next to Insert Primitive) draws a tube along a path and adds it to the active piece, in a face group of its own. Pieces > + Add > Tube (draw)… (or the command palette) does the same but makes the tube a new piece; the Tube window's Accept option switches between the two. Drag from the model, where the tube starts on its surface, or from empty space. The path lies flat to the view; orbit afterwards to move points in depth.\n\n\
- Drag a point to move it.\n\
- Shift+drag a point up or down to make the tube thicker or thinner there. Thickness blends smoothly between points.\n\
- Click the line between two points to add one. To remove a point, click it and press Delete (or Backspace), Alt+click it, or drag it onto the next point to merge them.\n\
- Double-click a point to make it a sharp corner (a square handle): the tube runs straight into it and out again. Double-click again for a smooth point. Corners are kept when you edit the tube again later.\n\
- Drag anywhere away from the tube's points and line to orbit around it.\n\
- While a tube is open, Undo and Redo (Ctrl/Cmd+Z) step through its own edits; undoing past its first edit closes it, and after that (or once it's accepted) they work on the scene again.\n\
- Profile (in the Tube window) sets the tube's cross-section: start from Round, Square, Triangle, Star, D-shape or Ribbon, then drag the points on the square to reshape it. Click the outline to add a point, right-click (or Alt+click) a point to remove it, and hold Shift to snap to the grid. All smooth draws a curve through the points and All sharp straight sides; double-click a point to make just that one a sharp corner (a square handle), and again to round it. Mirror left and right moves the matching point on the other side too. The faint circle is the tube's thickness. The profile turns with Twist, shapes braid and coil strands too, and the next tube starts with it.\n\
- The Tube window sets the thickness (of the selected point, or the whole tube), the number of sides, rounded ends, and a mirror copy across the X plane.\n\
- Twist turns the cross-section along the tube (easiest to see with few sides, or on a braid).\n\
- Spin coils the tube around its path like a spring: Coil size is the coil's radius, in tube thicknesses, and Coil turns how many times it goes round (negative winds the other way).\n\
- Style Braid makes three round strands woven along the path, inside the tube's thickness. Spread goes from tight (the strands touch) to loose (a strand's width of gap); Turn is how many weaves along the length, and more weaves make thinner strands. Twist and Spin work on a braid too. These stay editable with Edit tube…, and the next tube starts with the last shape used.\n\n\
Accept (or Enter) adds the tube as a new piece, in one undo step; Cancel (or Esc) discards it. To change it later, Pieces > Edit tube… reopens the active piece's last tube, as long as it hasn't been sculpted, remeshed or regrouped since; Update tube replaces it. To fuse it with the model, use Merge + remesh in the Pieces panel, which fuses the visible pieces into one surface with a voxel remesh; Join keeps them as they are, in one piece.",
    },
    Topic {
        title: "OxySpheres",
        keywords: "oxyspheres spheres armature skeleton base mesh block out blockout figure creature character limb joint skin mirror",
        body: "OxySpheres blocks out a shape with a tree of spheres, then turns it into one smooth mesh to sculpt. Pieces > + Add > OxySpheres… (or the command palette) opens one sphere in the middle of the view.\n\n\
- Drag from a sphere to pull out a new one. A click without dragging only picks a sphere, for the Radius slider.\n\
- Shift+drag a sphere to move it; Ctrl+drag it up or down to make it bigger or smaller. The window's Move and Size modes make that the plain drag. With Spheres below follow, moving a sphere moves the ones hanging from it.\n\
- Click the line between two spheres to put a sphere on it.\n\
- In Rotate mode, drag from a sphere or link to turn the branch around its parent. Choose Link as the axis to twist along the limb. With Mirror across X on, reflected branches turn together.\n\
- Alt+click a sphere, or pick it and press Delete, to remove it with the spheres below it. The first sphere stays.\n\
- Drag away from the spheres to orbit.\n\
- Mirror across X (on when X symmetry is) adds, moves, resizes and removes spheres on both sides of the plane through the first sphere.\n\
- Voxel skin can preview a coarser version of the mesh with Show the mesh. Quad base mesh previews the spheres and builds edge loops along chains and joints with up to six links when you choose Make mesh. Attach five fingers or toes and their incoming limb to the same joint. Give close links enough room to form separate loops. Use Subdivision to smooth the quad mesh.\n\
- Resolution sets voxel detail or the density of rings along the quad limbs. Blend sets how soft the joins are.\n\
- While the spheres are open, Undo and Redo step through their edits; undoing past the first edit closes them.\n\n\
The spheres are a piece of their own (an OxySpheres piece, with a sphere icon in the Pieces list), never used up. Make mesh (or Enter) adds a new piece made from them and keeps them, hidden out of its way; Make mesh again after changing them updates that mesh, as long as you haven't sculpted it, and once you have, it adds a new piece so your sculpting stays. Voxel skin makes thin parts finer automatically so they come out round. Keep spheres saves them without making a mesh. Cancel (or Esc) leaves them as they were or discards new ones. Each is one undo step. To change them again and make another mesh, pick the OxySpheres piece and click it, or use Pieces > Edit OxySpheres…, however much the meshes made from them were sculpted since. They are saved with the project.",
    },
    Topic {
        title: "OxyMorph (Beta)",
        keywords: "oxymorph morph blend fork variant preview mix shape",
        body: "Tools > OxyMorph (Beta) starts with the displayed mesh of the active piece (A). Choose Fork B, then Compare A / Mix / B. Sculpt either source in its own pane. Orbit, pan, zoom, and frame together. Use Keep A, Keep B, or Erase in the center to paint which variant to retain. Undo in the center reverts a paint stroke. Undo in an outer pane uses project history. Preview mix also works in the regular viewport. Create piece from mix makes a new sculptable piece in one Undo step and keeps A's material and mesh attributes. Keep the same topology and subdivision level while mixing. The current limit is 200,000 triangles. Save a result piece to keep it in the project. Closing the window discards the in-progress mix.",
    },
    Topic {
        title: "Poly Edit (model polygons)",
        keywords: "poly edit polygon modeling modes soft selection falloff proportional gizmo transform rotate scale faces edges points vertices select pick box lasso ellipse circle area extrude inset bevel chamfer bridge slide spin split equalize to circle circularize repeat crease smooth subdivide catmull clark subdivision surface sharp move delete faces edges points edge loop insert close hole weld reach group flat island hard surface low poly",
        body: "Tools > Poly Edit (model polygons)… models the active piece by its faces. It's for low-polygon pieces: a primitive from + Add (Divisions 1 or 2), a retopology mesh, or anything up to 400,000 triangles without subdivision levels. The Poly Edit window is resizable and scrolls when its content is taller than the window, so every control stays reachable on a short display.\n\nModes: press 1 for Faces, 2 for Edges, 3 for Points (or use the buttons at the top of the window). The window then shows only that kind's actions, and a click picks that kind: anywhere on a face, the nearest edge, the nearest point. 4 is Auto: what's under the pointer decides, and all actions show. Faces is the default; the keys pick brushes again once Poly Edit is closed. Ctrl/Cmd+click picks several elements of the current kind, and Ctrl/Cmd+drag paints the pick over the ones the pointer passes (started on a picked one, it takes them out instead); switching modes turns the pick into the new kind (a face's sides or corners, the faces that picked edges or points close). Under Select, choose Click, Box, Lasso or Ellipse: Box, Lasso and Ellipse drag an outline to select faces, edges or points through the piece. Turn on Front only to pick visible elements. Hold Shift for a circle with Ellipse. Plain drag replaces the selection, Ctrl/Cmd adds, Alt subtracts, and Escape cancels. Auto mode keeps click/paint select only. The transform gizmo works on the pick too: with something picked, press T (or choose All, Move, Rotate or Scale under Gizmo in the window) and drag its arrows, rings or cubes to move, turn or scale what's picked. Shift+drag extrudes picked faces or border edges first and moves the new ones (on a scale handle, it insets the faces); the new geometry stays picked, so you can keep extruding. Soft selection (under Gizmo) makes the gizmo also move the points near the pick, less the further they are, for smooth bulges and bends: Radius sets how far it reaches and Falloff how quickly it fades (soft to sharp); orange dots show the points it will move. The mode buttons are also at the top of the brush shelf while Poly Edit is open.\n\n\
- Point at a face: the faces the action would change light up in orange (red for Delete).\n\
- Extrude: drag to pull them out with walls around them (or push them in). Touching faces move out as one block; tick Each face on its own to extrude them separately.\n\
- Inset: drag right to put a rim of faces inside their edges; further for a wider rim.\n\
- Move: drag to slide them along their normals; the faces around stretch to follow. Delete: click to remove them, leaving a hole.\n\
- Bevel: like Extrude, but the cap also shrinks in from its edges, for a chamfered block. Equalize: click to even out the spacing of their points along the surface.\n\
- Bridge: click a face, then another with as many corners (it's outlined in green until the second click; Esc drops it). Both are removed and a tube of faces joins them: a handle, or a hole through. Rows and Twist set its shape.\n\
- Reach spreads the action: This face, Face loop (across the edge nearest the pointer), Face group, Face group (this island), Flat region (facing the same way within 5°) or Island (everything connected). Extruded caps, walls and inset rims get new face groups, so the next action can reach them by group.\n\
- Ctrl/Cmd+click faces to pick several (outlined in green; Ctrl/Cmd+click again to unpick). Ctrl/Cmd+double-click picks a whole loop: an edge's edge loop (on a hole's edge, the whole border), or a face's face loop across the nearest edge. Esc, or Ctrl/Cmd+click on empty space, clears the pick; a plain click or drag on empty space only orbits, so the pick stays. Dragging or clicking one of them then acts on all of them together, whatever the Reach.\n\
- To Circle: pick points, edges or faces, set Amount, then click To Circle to round the picked outline in its fitted plane. The pick stays active.\n\
- Knife: turn it on in the window, then click points on the model to draw a cut. Clicks snap to a corner or an edge when close to one; otherwise they land inside the face and become corners of the cut. The path follows the surface you see, adds a point wherever it crosses an edge and splits every face it crosses. Enter, Cut, or a click on the last point again cuts; Backspace takes the last point back, Esc clears the path (Esc again turns the Knife off), and clicking the first point again cuts a closed loop. A loop clicked entirely inside one face cuts a new face out of it, with a ring of faces joining it to the face's corners. An end inside a face carries the cut straight on to that face's edge (drawn dashed), so the face is split all the way across. One undo step.\n\
- Mirror across X does the same to the matching faces on the other side.\n\n\
- Edges: point near an edge and it lights up instead. Insert loop: click to cut a new loop across the quads (the edges it will cut light up), or drag along the edge to slide it first. Shift+click puts it exactly in the middle of the edge. Two loops cuts a pair, the same distance in from each end; dragging moves them in or out together (Shift+click: at thirds). Dissolve: remove the edge (or its loop or ring, with Reach) and merge the faces on each side. Delete: remove the faces touching the selected edges, leaving a hole or trimming the border. Extrude border: drag an edge of a hole or open surface outwards (Reach Edge loop takes the whole border). Close hole: click a hole's edge to fill it with one face. Bridge: click a border edge, then another, to join the two with a quad (Rows sets how many rows of faces; Esc drops the first click). Pick a row of border edges on each side first (Ctrl/Cmd+click) and click one of each to join the rows with a strip of quads; pick two whole hole borders (Ctrl/Cmd+double-click) to join the holes with a tube (Twist turns one end). Slide: drag across an edge (or its loop) to move it along the edges beside it. Bevel: drag to cut the edges off into strips of faces; where three bevelled edges meet, a corner face fills in. Bevel rows and Profile (shown while either Bevel is chosen) round a bevel over several rows, from flat (0) to round (1), for face bevels too. Spin: click to turn an edge one step within its two faces.\n\
- Points: point at a corner. Move: drag it across the screen, and drop it on another point to weld the two. Delete: remove it and the faces around it. Split face: click a point, then another of the same face, to cut the face in two between them.\n\n\
Smooth: Crease (for edges, or for every edge at a point) marks edges to stay sharp when the model is smoothed, at the Crease weight (100% stays sharp; lower rounds off after a level or two); click again to clear. Creased edges show in purple. Preview 1–3 shows the smooth surface under your polygons as you work. Subdivide (smooth) turns the polygons into that surface (all quads) and keeps modeling; Smooth and sculpt does the same and closes Poly Edit, ready to sculpt (Ctrl+D then adds subdivision levels).\n\n\
Y repeats the last action, with the same amount, on whatever is under the pointer. Each change is one undo step; Escape during a drag cancels it; otherwise Escape first drops a half-done bridge or split, then clears the pick, then (or Done) closes the tool. Drag empty space to orbit.",
    },
    Topic {
        title: "Retopology",
        keywords: "retopology retopo quads low poly topology draw game animation edge loop relax pin lock",
        body: "Tools > Retopology (draw quads)… draws a new, light mesh of quads over the active piece (the surface). Every point you place lands on the surface.\n\n\
- Click to place a point. Drag a point, edge or face to slide it along the surface; drop a point on another to weld them.\n\
- Hold Shift: the quad the nearest points make shows in green (a triangle in purple). Shift+click makes it. Shift+drag relaxes the points under the cursor.\n\
- Tab+drag a border edge to pull out a new quad. Ctrl+click an edge to insert an edge loop across it. Ctrl+Shift+click deletes a point or face.\n\
- In Contour mode, drag successive cuts across a limb to build rings and connect them with quads. Choose New strip before starting a separate run.\n\
- Alt+click a point, edge or face to pin its points (shown as orange squares): relaxing leaves them where they are, though you can still drag them. Alt+click again, or Unpin all in the window, to free them.\n\
- Mirror across X (in the window, on when X symmetry is) repeats everything on the other side; points near the centre snap onto it.\n\n\
Subdivide and project (in the window) adds subdivision levels moved onto the sculpt: the top level carries its detail, level 1 stays your quads. The first face makes a new piece, Retopo. Each action is one undo step, it is saved with the project, and its quads export as quads (OBJ). Done (or Esc) closes the tool; open it again with the sculpt or the Retopo piece active to carry on.",
    },
    Topic {
        title: "Blender bridge",
        keywords: "blender bridge send back link round trip addon export import",
        body: "The OxyBrush Bridge add-on for Blender (in the integrations/blender folder of OxyBrush's source) sends meshes here and takes them back. In Blender, set the OxyBrush program in the add-on's preferences, select mesh objects and choose Object > Send to OxyBrush. They open here as pieces, placed as they were, and the top bar shows Linked to Blender.\n\n\
- File > Send back to Blender (or click the badge) returns every piece. Objects whose vertices you didn't add or remove keep their UVs, materials, modifiers and shape keys in Blender; others get new mesh data; pieces you made here arrive as new objects.\n\
- File > Send back to Blender and close does the same, then starts a new document.\n\n\
Dynamic detail, remesh, subdivision and Close holes change the vertices, so those objects come back as new mesh data.",
    },
    Topic {
        title: "Lights and shadows",
        keywords: "lights lighting shadows sun spot area soft box lamp viewport shading render matcap softness",
        body: "View > Shading > Light the viewport shades the model with lights instead of the material; View > Shading > Lights… sets them up. There are four lights: each can be a Sun (a direction), a Point (a position), a Spot (a cone aimed at a point) or an Area light (a round light with a size, facing a point, like a soft box), with its own color, brightness and shadows. An area light's shadows are sharp where things touch and softer further away; a bigger light gives softer light and shadows. Softness sets how gently each light fades where the surface turns away from it, and how soft its shadow edges are. Follow the view keeps a light in place relative to you, so the model turns under it as you orbit.\n\nLight the matcap keeps your matcap as the material: the lights brighten it where they shine and their shadows darken it.\n\n\
While the Lights window is open, numbered dots show the lights in the viewport: drag one to move its light (a sun's dot sets its direction; a spot's diamond sets where it aims). Shadows come from suns, spots and area lights; each one redraws the model once more per frame, and an area light's soft shadows cost a little more to draw, so turn shadows off for lights that don't need them on very dense meshes.",
    },
    Topic {
        title: "Remesh and decimate",
        keywords: "topology voxel rebuild reduce polygons triangles resolution shell hollow thickness wall holes project detail reproject shrinkwrap",
        body: "Voxel remesh (Ctrl+R) rebuilds the active piece with even triangles, so stretched areas sculpt well again. Resolution sets how fine the new mesh is.\n\n\
A closed model is filled solid. A model with holes has them patched first, so it comes out solid too; an open sheet (a plane, a flat patch) has nothing inside to fill, and the status bar says so. For a wall instead, tick Shell: set its Thickness (in scene units; a standard-size piece is 2 across) and which way the wall goes from the surface (Outward, Both sides or Inward). A sheet gets that thickness; a closed model comes out hollow. Make the wall at least two voxels thick, or raise the resolution.\n\n\
OxyRemesh in the Topology panel generates a quad-dominant mesh. It tests fitted charts with integer-aligned seams and falls back to other extraction paths if coverage or topology checks fail. A closed sculpt needs a closed result.\n\
\n\
Decimate reduces the triangle count to the Keep percentage while preserving the shape, for export or to lighten a heavy piece. These operations run in the background and can be undone.\n\n\
**Project detail** (Mesh menu, Topology > Project detail, or the command palette) moves every vertex of the active piece onto the closest point of the other visible pieces, so a clean remesh or cage picks up a high-detail sculpt without changing its triangles. Mesh > Project onto… picks one source piece instead. Subdivision levels are removed; one undo step. For projecting under the brush only, use the Scene Project brush. Retopology's Subdivide and project still builds subdivision levels on a drawn quad mesh.",
    },
    Topic {
        title: "Saving, autosave and recovery",
        keywords: "save project oxb autosave crash recover recent files open close revert reload discard",
        body: "Ctrl+S saves the whole scene as an .oxb project (pieces, levels, masks and groups). File > New starts over with a sphere, cube, plane, human base mesh, or an empty scene with no pieces. Saves are atomic: a crash while saving never damages the previous file.\n\n\
OxyBrush also autosaves every couple of minutes (Settings > Autosave). If it ever closes unexpectedly, the next start offers to recover your work. File > Open recent and the welcome window list the files you used last. File > Revert reloads the project as you last saved it, dropping the changes since then (it asks first; Undo can't bring them back). File > Close project closes the project (asking to save unsaved work first) and goes back to the start screen.",
    },
    Topic {
        title: "Importing and exporting",
        keywords: "export import obj stl gltf glb ply 3mf 3d print file other programs game engine lod split cli units scale size millimetres mtl material",
        body: "Ctrl+I imports one or more glTF (.glb, .gltf), OBJ, STL or PLY files. Shift-click or Ctrl/Cmd-click in the dialog to pick several, or drop several files on the window. Each object in an OBJ file, and each mesh in a glTF scene (placed where the scene puts it), becomes its own piece, and every file from one import is a single undo step. A file that can't be read is skipped and named in the status bar; the others still come in. Files are added in name order, because the file dialog doesn't promise the order you clicked. Materials and textures are ignored, but UVs (texture coordinates) are kept: through sculpting and saving, and in OBJ and glTF exports, so the model can go on to texturing. Rebuilding the triangles (voxel remesh, dynamic detail, decimate, subdivision levels, Close holes) drops UVs for now; a note says so and Undo brings them back. glTF files compressed with meshopt or quantization open as usual; Draco-compressed ones can't be read, so export those without Draco.\n\nMesh files don't say what units they use. If an import is more than 8 times larger or smaller than what's already in the scene (a model in millimetres, say), it's scaled by a power of ten to fit, and later imports into the same project get the same scale so parts still fit together. Several files imported together share that one factor, taken from the whole batch's size, so a small part stays the right size next to a large one. One Undo keeps the original size.\n\n Ctrl+E exports every visible piece (hidden faces are left out): glTF/GLB keeps each piece as its own object for game engines and other 3D tools, OBJ writes one object per piece, and STL and PLY combine them (STL is the usual choice for 3D printing).\n\n\
- Pieces > Make LOD chain adds lighter copies at 1/2, 1/4 and 1/8 of the triangles.\n\
- Pieces > Split by face groups or Split into loose parts turns one piece into several.\n\
- The oxybrush-cli command-line tool converts, remeshes, decimates, makes LODs and splits files without opening the app.\n\nImport inspection checks each imported piece for open and non-manifold edges, inconsistent winding, and duplicate or degenerate triangles. When it finds something, click Inspect import in the status bar. Frame piece takes you to that piece. Open edges can be intentional on a sheet. Repair faces removes duplicates and zero-area triangles and corrects winding. Choose Close holes separately to patch open edges on a suitable piece. Each repair updates the report and takes one Undo step.",
    },
    Topic {
        title: "Layouts and panels",
        keywords: "interface layout touch compact tablet arrange panels scale theme",
        body: "Settings > Layout switches between Standard, Compact (small screens), Touch friendly (bigger controls and on-screen Smooth/Mask/Invert buttons for tablets), Workstation, and Focus (Pieces + Brush only in the settings panel). Settings > Arrange panels moves the brush shelf, the settings panel and the size/strength bar.\n\n\
View > Theme… holds everything about the look in one window: the themes (including Studio, flat neutral greys with bar sliders, the navy Midnight theme and a high-contrast theme) and looks that set a theme with matching colors and corners, a Mix that takes the colors, the controls (Studio's bar sliders, Sculptor's clay sliders and card tiles) and the shadows each from any theme, the accent color, a custom button color, the brush icons' color (clay, your accent or your own) and the viewport background, the interface scale, corner rounding and slider thickness (Thick bars are easier to hit with a pen). All of these are remembered.\n\n\
The Touch layout has no mouse hover. Press and hold a control to read what it does.",
    },
    Topic {
        title: "Shading and materials",
        keywords: "material matcap ambient occlusion cavity view look flat faceted hard edge angle custom image background gradient color backdrop",
        body: "View > Material picks the look of the clay. Ambient occlusion darkens folds and the inside of shapes, and cavity shading darkens creases and pores, so detail reads clearly; set both in View > Shading, or toggle them with the ball button in the view strip.\n\n\
- Each piece can have its own material: in the material picker (or View > Material) set For to the piece (its name, then only), then pick one; it applies to the active piece and is saved with the project. Use the scene's material puts it back. Whole scene sets the material every other piece shows. Up to 8 different piece materials show at once; with viewport lights on, all pieces use the lit surface, unless Light the matcap is on (Lights window), which keeps each piece's material under the lights.\n\
- Load matcap… (in the material picker or the View menu) uses your own PNG or JPEG image of a lit sphere; it is kept for next time. Right-click it in the picker to remove it.\n\
- View > Flat shading shows every polygon as a flat facet.\n\
- View > Shading > Hard edge angle sets how sharp a corner must be before the faces there shade flat (40° by default). 180° (Off) keeps every edge smooth. The view updates when you let go of the slider. It is remembered, and it is not saved in the project.\n\
- View > Wireframe (Shift+W or the wireframe icon in the view strip) overlays polygon edges atop the shaded surface, with adjustable line opacity.\n\
- View > Shading > Floor grid draws a grid on the floor under the model. It stays at the bottom of the visible pieces and its spacing follows the model's size.\n\
- View > Background (also under Settings) switches between a soft gradient and one flat color. Colors… sets the center and the edge, or the single flat color. Reset to default puts the original gradient back. The command palette can open the colors, switch the style or reset it. The choice is remembered.",
    },
    Topic {
        title: "Updates and privacy",
        keywords: "update version network privacy telemetry",
        body: "OxyBrush never contacts the internet unless you turn on update checks in Settings > Updates (or use Help > Check for updates). Then it asks GitHub for the latest release and sends nothing about you or your work. This guide is built in and works offline.\n\n\
Settings > Local AI agent (off by default) listens only on this computer, so an agent can sculpt with you. It does not call out to a model or any other host. See the MCP guide shipped with OxyBrush.",
    },
    Topic {
        title: "AI agents",
        keywords: "mcp agent ai local tandem undo token",
        body: "Settings > Local AI agent lets a program on this computer sculpt in the same scene as you. It is off until you turn it on. Copy connection gives the address (127.0.0.1 only) and a token that changes every launch.\n\n\
- While an agent is connected, the status bar says so. Disconnect drops it and changes the token.\n\
- The agent's edits are ordinary undo steps labeled Agent:, so Ctrl+Z removes them.\n\
- Python plugins talk to OxyBrush the same way, but each run gets a connection of its own: running one doesn't turn this setting on, the status bar shows the plugin's name while it runs, and its edits are undo steps named after it (Plugin: name:).\n\
- A screenshot the agent takes is the viewport, and screen strokes use those pixels.\n\n\
Nothing is uploaded. The listener never accepts a connection from another machine.",
    },
    Topic {
        title: "Focal length",
        keywords: "fov field of view lens camera mm perspective zoom millimeters",
        body: "View > Focal length, the lens button beside the viewport, or the command palette (Ctrl+K, then type \"focal\") sets the perspective lens. The number is a 35mm-equivalent focal length, the same kind of millimeter number a camera lens uses.\n\n\
- 18 mm is wide and stretches depth. 50 mm looks natural. 135 mm flattens it, so a nose and an ear look closer in size.\n\
- Drag the slider or type a value from 15 to 200 mm, in hundredths. The vertical field of view in degrees is shown next to the number; hover it for the horizontal angle at this window's shape.\n\
- The five presets jump to those lenses. Reset puts back the original view (35° vertical, about 38 mm).\n\n\
The model stays about the same size when the lens changes. The mouse wheel is still zoom, and it is separate from the lens. Orthographic view (O) ignores the lens entirely; the choice applies again when you switch back to perspective, and it is remembered next time you open OxyBrush.",
    },
    Topic {
        title: "Keymap",
        keywords: "hotkey shortcut rebind mouse navigation preset snap",
        body: "Settings > Keymap lists every command, brush and view key. Click a key and press a new one, add a second key with +, or reset one command. Conflicts are marked; the first command in the list wins until you change one of them.\n\n\
- OxyBrush is the original layout: drag empty space to orbit, right-drag to pan, the wheel to zoom, Shift to snap the orbit to 45°.\n\
- MMB orbit uses the middle button to orbit, Ctrl+middle to zoom and Shift+middle to pan. The 45° snap key starts as Alt and can be changed.\n\
- Ctrl carves, Alt masks: Ctrl inverts the brush, Alt+drag on the mesh masks, Ctrl+Alt erases, Alt+click on empty space inverts the mask and Alt+drag on empty space clears it.\n\n\
Keys are remembered next to your other settings. Export writes a keymap file you can share; Import reads one back. The touch layout's Smooth, Mask and Invert buttons keep those jobs even when the keyboard keys move. View keys start on the numpad (front, right, top) and can be moved to any key, so a keyboard without a numpad can still snap the view. Back, left and bottom start unbound.",
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
            crate::tips::hover(
                ui.add(
                    egui::TextEdit::singleline(&mut g.query)
                        .hint_text("Search the guide…")
                        .desired_width(f32::INFINITY),
                ),
                crate::tips::plain::GUIDE_SEARCH,
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
                            crate::tips::hover(
                                ui.selectable_value(&mut g.topic, i, TOPICS[i].title),
                                crate::tips::plain::GUIDE_TOPIC,
                            );
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
        let mut lasso: Vec<_> = lasso.iter().map(|&i| TOPICS[i].title).collect();
        lasso.sort_unstable();
        assert_eq!(
            lasso,
            ["Face groups and hiding", "Masking", "Poly Edit (model polygons)", "Symmetry"],
            "masking, visibility lassos, Poly Edit area pick, and symmetry for mirrored mask lassos"
        );
        assert!(search("stl export").iter().any(|&i| TOPICS[i].title == "Importing and exporting"));
        assert!(search("cursor").iter().any(|&i| TOPICS[i].title == "Brush cursor"));
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
