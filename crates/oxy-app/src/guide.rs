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
- Tab hides the panels to give the model the whole window.\n\n\
Prefer starting strokes beside the model? Turn on Settings > Strokes can start on empty space and orbit with the middle mouse button instead.\n\n\
Every shortcut and the mouse buttons can be changed in Settings > Keymap, including switching between the Default and Single-key tools shortcut schemes. The Right-drag orbit preset orbits with the right button; the MMB orbit preset uses the middle button to orbit, Ctrl+middle to zoom and Shift+middle to pan.",
    },
    Topic {
        title: "Sculpting basics",
        keywords: "start first stroke clay brush size strength radius",
        body: "Drag on the model to sculpt with the current brush. Raise adds material and Carve removes it; hold Alt to flip for one stroke.\n\n\
- [ and ] make the brush smaller and larger; - and = change its strength. The size/strength bar does the same with the mouse, and so does the quick menu: right-click the model or hold Q.\n\
- The brush size is on screen: it stays the same size as you zoom, so zooming in gives finer strokes. Brush settings > Size > On the model makes it a size on the model instead (in scene units, for all brushes): zoom in and the circle grows with the model, so a stroke covers the same area at any zoom.\n\
- S, U and I open a slider right under the pointer for size, strength and Paint strength; Shift+O opens one for focal shift (O in the Single-key tools scheme). Press the key again, Esc, or click outside to close it.\n\
- Hold Shift to smooth with any brush, Ctrl to mask.\n\
- With a pen, pressure sets the strength (and the radius, if you turn that on in the brush settings).\n\
- Ctrl+Z undoes a stroke, Ctrl+Shift+Z redoes it. Ctrl+Alt+Z undoes on the active piece only, skipping changes to other pieces (Preferences > General can make that the default).",
    },
    Topic {
        title: "Pen tablets and stylus",
        keywords: "tablet pen stylus side button barrel windows ink pressure curve sensitivity firm soft wacom huion kamvas xppen x11 wayland linux cachy scroll panel hold gesture",
        body: "OxyBrush supports pressure-sensitive stylus and drawing tablets (Wacom, Huion, XP-Pen, etc.).\n\n\
- Stylus pressure controls brush strength, and can also modulate brush radius when enabled in the brush settings.\n\
- Preferences > Pen & tablet sets how pressure feels: drag the two points on the curve (an S shape gives fine control at both ends; double-click to straighten), the curve bend, Lightest touch (the strength of a very light stroke) and Full pressure at (how hard you press for full strength), with Soft, Linear and Firm presets. While you draw, a dot shows your pen on the curve. If your tablet driver has its own pressure curve, keep that one straight so the two don't add up.\n\
- In panels and lists, drag with the pen to scroll (or drag with a side button held), and hold the pen still on a control for a moment to open its menu, like a right-click, and read its tip. Preferences > Pen & tablet sets when dragging scrolls: with a pen, always (for pens whose drivers send no pressure) or never.\n\
- In the viewport, the Pen side buttons navigation preset (Preferences > Keymap, or the button in Pen & tablet) makes the lower side button orbit, with Shift pan and with Ctrl zoom, and the upper side button pan.\n\
- Pen side buttons can be mapped to right-click (quick menu), middle-click (navigation/scrolling), or modifier keys in your tablet driver.\n\
- On Windows with Windows Ink, the pen's side button already acts as the right mouse button: a tap opens the quick menu, and with the Right-drag orbit preset (Settings > Keymap) holding it while you drag orbits the view and Ctrl + side button zooms.\n\
- On Linux under Wayland compositors (CachyOS, KDE Wayland, GNOME Wayland, Hyprland), drawing tablets require the X11/XWayland backend because native Wayland tablet events are not exposed to winit. OxyBrush automatically detects connected tablets and defaults to the X11 backend when available. You can also manually force X11 mode by starting with `oxybrush --x11` (or `oxybrush --backend x11`), or by setting `OXY_BACKEND=x11`.",
    },
    Topic {
        title: "Brush cursor",
        keywords: "ring circle orientation surface normal tangent align screen",
        body: "The ring under the pointer is the brush cursor. It shows the brush size, and the inner ring is where the falloff is about halfway. Focal shift (in the brush settings, or Shift+O) moves the inner ring out for a firmer brush and in for a softer one.\n\n\
By default the ring lies flat on the screen, so it stays a circle at any viewing angle. View > Align cursor to surface (the same checkbox is in Settings, and the command palette finds it too) lays the ring on the clay instead: a circle the size of the brush, flat against the surface, with a short line showing which way the surface faces.\n\n\
That direction is an average of the surface around the pointer, eased as you move, so on a dense mesh it stays steady instead of twitching from one triangle to the next. Move the pointer off the model and the ring goes back to lying flat on the screen. The choice is remembered.",
    },
    Topic {
        title: "Choosing brushes",
        keywords: "brush picker shelf number keys space find",
        body: "Click a brush in the shelf, press its number key (1–9 and 0 select the first ten), or press Space and type part of its name: \"cs\" finds Clay Strips, Enter picks it.\n\n\
Drag brushes in the shelf to put the ones you use most first; the number keys follow the new order. Hover any control for a short explanation and its shortcut. On a touch screen, press and hold the control: the same text stays up after you lift your finger, and the control does not activate.\n\n\
- Grab, Topological Move and Snake Hook: hold Alt to move the surface along its normal.\n\
- Kelvin Grab, Kelvin Twist and Kelvin Scale: elastodynamic Kelvinlet brushes (de Goes & James, 2017) for volume-preserving elastic deformation, twisting, and radial expansion/pinch.\n\
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
- Curve draws a stroke that stays editable: drag its points to reshape it, click the curve to add a point, Alt+click a point to remove it. Size, strength and the Fade in / Fade out sliders update it live. Accept (Enter) applies it as one undo step, as does starting the next stroke; Cancel (Esc) discards it. You can orbit around it while it is open.\n\
- In Freehand mode, hold the pen or mouse still for about half a second while drawing: a ring fills up at the cursor and the stroke becomes a curve; keep moving to extend it. Turn this off with Hold to adjust in the Stroke section, or tick Ask first to be asked each time.\n\
- Edit > Last stroke reopens the Freehand stroke you just made: Edit as curve (same brush, editable, Enter applies it), Use as Curve Move curve, or Use as tube path. It works right after the stroke, even if you've turned the view since.\n\
- While a curve is open, click a point to give it its own size and strength (a ring shows the size), tick Join the ends for a loop, and use Ctrl+Z / Ctrl+Shift+Z to undo and redo your curve edits.\n\n\
The Spray slider scatters dabs around the stroke. The Stabilizer slider makes the brush trail behind the pointer for smooth, controlled lines. Alphas shape each dab: pick one in the Alpha section, or load your own grayscale PNG.\n\nCapture from view (in the Alpha section, or the command palette) makes an alpha from what you see: a depth image of the model in the middle of the view, nearest parts white, cropped to the model, at the size picked next to the button (up to 4096 px; bigger gives smoother edges). Sculpt or arrange a detail (a rivet, a scale, a button), look at it straight on, capture it, then stamp it anywhere with Stamp strokes. Captured and loaded alphas are saved with the project, and come back when you open it. Export alpha… saves the selected alpha as a 16-bit grayscale PNG, to use in other projects or edit in a 2D app and load it back with Load image….",
    },
    Topic {
        title: "Symmetry",
        keywords: "mirror x y z radial",
        body: "X symmetry is on by default: strokes on one side are mirrored to the other, and so are mask boxes and lassos. Toggle it with X or in the toolbar; Y and Z are in the Symmetry section, together with radial symmetry (repeat a stroke around an axis, for wheels, flowers or ornaments).\n\nEach piece mirrors about its own centre, chosen from its shape when it's imported or opened; a model built around the origin keeps the origin. If mirroring lands in the wrong place (say, after moving the whole piece), click Recentre in the Symmetry section. Turning a mirror on briefly outlines its plane.\n\nSymmetry mirrors new strokes; it doesn't fix a model whose sides already differ. For that, Mesh > Symmetrize keeps one half (for example +X onto −X: the +X side is where the axis gizmo's red X points), replaces the other half with its mirror image and welds the two along the middle, about the piece's centre. It's one undo step and removes subdivision levels.",
    },
    Topic {
        title: "Deform window",
        keywords: "deform bend twist taper skew flatten squeeze sag inflate spherize noise polish relax smooth unify",
        body: "Mesh > Deform… changes the whole piece at once: bend, twist, taper, skew, flatten, squeeze, sag, inflate, spherize, noise, smooth, polish, relax and more.\n\n\
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
- Ctrl+drag on empty space masks a rectangle; press L to switch to a lasso. With symmetry on, the mirrored area is masked too.\n\
- Ctrl+click on empty space inverts the mask (with no mask, that masks everything). A Ctrl box around empty space clears it.\n\
- Ctrl+Shift+I inverts the mask, Alt+M clears it, M shows or hides it. Mask > Fill masks everything. Invert and Fill work on what's visible: hidden parts keep their mask.\n\
- The Mask menu can also grow, shrink and sharpen the mask, or mask cavities or peaks automatically. Mask border masks the edge of an open surface or of what's left visible after hiding part of it; Mask group borders masks the lines where face groups meet. Grow the mask to widen either.\n\
- Split masked moves the masked part into a new piece (Invert first to split off the unmasked part). Extract mask… makes a new shell from the masked area and leaves the piece whole.",
    },
    Topic {
        title: "Face groups and hiding",
        keywords: "polygroups groups visibility hide show isolate",
        body: "Face groups split a piece into named regions, shown as colors when G is on. Make them from the mask (Ctrl+W; with nothing masked it groups what's visible), from what's visible, or from loose parts (Groups menu).\n\n\
- Ctrl+Shift+click a group to show only it; Ctrl+Shift+click empty space to show everything.\n\
- Ctrl+Shift+drag shows only what's inside the selection; add Alt to hide it instead.\n\
- Hidden faces are not sculpted or exported. Mesh > Delete hidden removes them for good, and Mesh > Split hidden moves them into a piece of their own.\n\
- Mesh > Close holes fills every opening in the piece (after deleting faces, a cut, or an import with gaps) with a smooth patch in a new face group, so the piece is closed again, as 3D printing needs.",
    },
    Topic {
        title: "Pieces",
        keywords: "objects parts scene add duplicate merge join combine split detach solo",
        body: "A scene can hold several pieces (separate meshes), listed in the Pieces section. Sculpting affects the active piece; Alt+click another piece to make it active.\n\n\
- + Add opens a window for a sphere, cube, plane, cylinder, cone or torus: set its size and segments (low-poly by default, or tick Even triangles to remesh it for sculpting), then Accept; Duplicate and Mirror duplicate copy the active piece.\n\
- The eye button hides a piece; Alt+S shows only the active one.\n\
- Join combines the active piece and the other visible pieces into one piece, exactly as they are: nothing is remeshed or welded, and each keeps its own face groups, so you can still hide or select it. Merge + remesh instead fuses them into one closed surface with new, even triangles (a voxel remesh).\n\
- Mesh > Split masked (also in the Mask menu) moves the masked part of the active piece into a new piece; invert the mask first to split off the unmasked part instead.",
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
Masked parts stay put and partly masked parts move partly, so a soft mask edge bends smoothly: mask the body, leave an arm unmasked, and rotate it at the shoulder. The pivot sits on the edge of the mask.\n\n\
The fields under the gizmo show how far it has moved, turned or scaled since the gizmo opened, and follow along while you drag. To land on an exact value, type it and press Enter: typing 2 in Scale makes it twice the size it started at. Apply keeps the changes and counts from zero again; Cancel puts them back. Done closes the gizmo and keeps your changes. If you'd rather Done put back anything you didn't apply, like Cancel, turn on Preferences > General > Closing the gizmo puts back changes not applied (Redo brings changes back either way). Operators (mirror, put on the ground, centre at origin) are applied at once.",
    },
    Topic {
        title: "Bending with Curve Move",
        keywords: "curve move bend pose arm tail limb deform stretch wire",
        body: "Curve Move bends and stretches a long part (an arm, a tail, a horn) along a curve. Pick it from the brushes, draw a curve along the part, then drag the curve's points: the part follows, bending smoothly. With Through the middle on, the curve runs through the part's core rather than over its skin.\n\n\
Size sets how far from the curve the model follows: make the circle cover the part's thickness so it moves as a whole. Masked areas never move, and symmetry moves the mirrored side too. Click the curve to add a point, Alt+click to remove one, select a point to change its reach and strength there, and Reset to start over. Enter applies it as one undo step; Esc cancels.",
    },
    Topic {
        title: "Subdivision levels",
        keywords: "multires multiresolution detail divide levels",
        body: "Ctrl+D adds a subdivision level: four times the triangles, for finer detail. D and Shift+D move up and down between levels. Make broad changes on a low level and fine detail on a high one; the detail follows when you go back up.\n\n\
Remesh and decimate rebuild the mesh, so they remove the levels (undo brings them back).",
    },
    Topic {
        title: "Dynamic detail",
        keywords: "dynamic topology detail triangles adaptive refine sphere sketch",
        body: "Turn on Dynamic detail in the Topology section (or from the command palette) and sculpting brushes add triangles where they need more detail, and remove ones that have become too small. Start from a plain sphere and pull out shapes with Snake Hook, build up with Clay, without remeshing.\n\n\
How fine it gets depends on the Detail slider and the brush size: a smaller brush gives smaller triangles. Mask, Paint, the Grab-like brushes and the cutting tools don't change triangles. Each stroke is one undo step. It is off on pieces with subdivision levels, and works best up to a few hundred thousand triangles.",
    },
    Topic {
        title: "Tubes",
        keywords: "tube pipe horn tentacle tail limb cable path curve sweep draw",
        body: "The Tube brush (in the brush shelf, next to Insert Primitive) draws a tube along a path and adds it to the active piece, in a face group of its own. Pieces > + Add > Tube (draw)… (or the command palette) does the same but makes the tube a new piece; the Tube window's Accept option switches between the two. Drag from the model, where the tube starts on its surface, or from empty space. The path lies flat to the view; orbit afterwards to move points in depth.\n\n\
- Drag a point to move it.\n\
- Shift+drag a point up or down to make the tube thicker or thinner there. Thickness blends smoothly between points.\n\
- Click the line between two points to add one; Alt+click a point to remove it.\n\
- The Tube window sets the thickness (of the selected point, or the whole tube), the number of sides, rounded ends, and a mirror copy across the X plane.\n\n\
Accept (or Enter) adds the tube as a new piece, in one undo step; Cancel (or Esc) discards it. To change it later, Pieces > Edit tube… reopens the active piece's last tube, as long as it hasn't been sculpted, remeshed or regrouped since; Update tube replaces it. To fuse it with the model, use Merge + remesh in the Pieces panel, which fuses the visible pieces into one surface with a voxel remesh; Join keeps them as they are, in one piece.",
    },
    Topic {
        title: "Poly Edit (model polygons)",
        keywords: "poly edit polygon modeling box extrude inset move delete faces edges points edge loop insert close hole weld reach group flat island hard surface low poly",
        body: "Mesh > Poly Edit (model polygons)… models the active piece by its faces. It's for low-polygon pieces: a primitive from + Add (Divisions 1 or 2), a retopology mesh, or anything up to 400,000 triangles without subdivision levels.\n\n\
- Point at a face: the faces the action would change light up in orange (red for Delete).\n\
- Extrude: drag to pull them out with walls around them (or push them in). Touching faces move out as one block; tick Each face on its own to extrude them separately.\n\
- Inset: drag right to put a rim of faces inside their edges; further for a wider rim.\n\
- Move: drag to slide them along their normals; the faces around stretch to follow. Delete: click to remove them, leaving a hole.\n\
- Reach spreads the action: This face, Face loop (across the edge nearest the pointer), Face group, Face group (this island), Flat region (facing the same way within 5°) or Island (everything connected). Extruded caps, walls and inset rims get new face groups, so the next action can reach them by group.\n\
- Mirror across X does the same to the matching faces on the other side.\n\n\
- Edges: point near an edge and it lights up instead. Insert loop: click to cut a new loop across the quads (the edges it will cut light up), or drag along the edge to slide it first. Delete: remove the edge (or its loop or ring, with Reach), merging the faces on each side. Extrude border: drag an edge of a hole or open surface outwards (Reach Edge loop takes the whole border). Close hole: click a hole's edge to fill it with one face.\n\
- Points: point at a corner. Move: drag it across the screen, and drop it on another point to weld the two. Delete: remove it and the faces around it.\n\n\
Each change is one undo step; Escape during a drag cancels it, and Escape otherwise (or Done) closes the tool. Drag empty space to orbit. Bevel and bridge are coming next.",
    },
    Topic {
        title: "Retopology",
        keywords: "retopology retopo quads low poly topology draw game animation edge loop relax pin lock",
        body: "Mesh > Retopology (draw quads)… draws a new, light mesh of quads over the active piece (the surface). Every point you place lands on the surface.\n\n\
- Click to place a point. Drag a point, edge or face to slide it along the surface; drop a point on another to weld them.\n\
- Hold Shift: the quad the nearest points make shows in green (a triangle in purple). Shift+click makes it. Shift+drag relaxes the points under the cursor.\n\
- Tab+drag a border edge to pull out a new quad. Ctrl+click an edge to insert an edge loop across it. Ctrl+Shift+click deletes a point or face.\n\
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
        keywords: "lights lighting shadows sun spot lamp viewport shading render",
        body: "View > Shading > Light the viewport shades the model with lights instead of the material; View > Shading > Lights… sets them up. There are four lights: each can be a Sun (a direction), a Point (a position) or a Spot (a cone aimed at a point), with its own color, brightness and shadows. Follow the view keeps a light in place relative to you, so the model turns under it as you orbit.\n\n\
While the Lights window is open, numbered dots show the lights in the viewport: drag one to move its light (a sun's dot sets its direction; a spot's diamond sets where it aims). Shadows come from suns and spots; each one redraws the model once more per frame, so turn shadows off for lights that don't need them on very dense meshes.",
    },
    Topic {
        title: "Remesh and decimate",
        keywords: "topology voxel rebuild reduce polygons triangles resolution",
        body: "Voxel remesh (Ctrl+R) rebuilds the active piece with even triangles, so stretched areas sculpt well again. Resolution sets how fine the new mesh is.\n\n\
Decimate reduces the triangle count to the Keep percentage while preserving the shape, for export or to lighten a heavy piece. Both run in the background and can be undone.",
    },
    Topic {
        title: "Saving, autosave and recovery",
        keywords: "save project oxb autosave crash recover recent files open close",
        body: "Ctrl+S saves the whole scene as an .oxb project (pieces, levels, masks and groups). Saves are atomic: a crash while saving never damages the previous file.\n\n\
OxyBrush also autosaves every couple of minutes (Settings > Autosave). If it ever closes unexpectedly, the next start offers to recover your work. File > Open recent and the welcome window list the files you used last. File > Close project closes the project (asking to save unsaved work first) and goes back to the start screen.",
    },
    Topic {
        title: "Importing and exporting",
        keywords: "export import obj stl gltf glb ply 3d print file other programs game engine lod split cli units scale size millimetres mtl material",
        body: "Ctrl+I imports an OBJ or STL file; each object in an OBJ file becomes its own piece. Materials in OBJ files are ignored.\n\nMesh files don't say what units they use. If an import is more than 8 times larger or smaller than what's already in the scene (a model in millimetres, say), it's scaled by a power of ten to fit, and later imports into the same project get the same scale so parts still fit together. One Undo keeps the original size.\n\n Ctrl+E exports every visible piece (hidden faces are left out): glTF/GLB keeps each piece as its own object for game engines and other 3D tools, OBJ writes one object per piece, and STL and PLY combine them (STL is the usual choice for 3D printing).\n\n\
- Pieces > Make LOD chain adds lighter copies at 1/2, 1/4 and 1/8 of the triangles.\n\
- Pieces > Split by face groups or Split into loose parts turns one piece into several.\n\
- The oxybrush-cli command-line tool converts, remeshes, decimates, makes LODs and splits files without opening the app.",
    },
    Topic {
        title: "Layouts and panels",
        keywords: "interface layout touch compact tablet arrange panels scale theme",
        body: "Settings > Layout switches between Standard, Compact (small screens) and Touch friendly (bigger controls and on-screen Smooth/Mask/Invert buttons for tablets). Settings > Arrange panels moves the brush shelf, the settings panel and the size/strength bar.\n\n\
View > Theme… holds everything about the look in one window: the themes (including Studio, flat neutral greys with bar sliders, the navy Midnight theme and a high-contrast theme) and looks that set a theme with matching colors and corners, the accent color, a custom button color, the brush icons' color (clay, your accent or your own) and the viewport background, the interface scale, corner rounding and slider thickness (Thick bars are easier to hit with a pen). All of these are remembered.\n\n\
The Touch layout has no mouse hover. Press and hold a control to read what it does.",
    },
    Topic {
        title: "Shading and materials",
        keywords: "material matcap ambient occlusion cavity view look flat faceted custom image background gradient color backdrop",
        body: "View > Material picks the look of the clay. Ambient occlusion darkens folds and the inside of shapes, and cavity shading darkens creases and pores, so detail reads clearly; set both in View > Shading, or toggle them with the ball button in the view strip.\n\n\
- Load matcap… (in the material picker or the View menu) uses your own PNG or JPEG image of a lit sphere; it is kept for next time. Right-click it in the picker to remove it.\n\
- View > Flat shading shows every polygon as a flat facet.\n\
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
        assert_eq!(lasso, ["Masking", "Symmetry"], "masking, and symmetry for mirrored mask lassos");
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
