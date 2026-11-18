# RFC 0027: Tabbed projects

Status: Draft

## Summary

One OxyBrush window can have several projects open, one per tab. Switching
tabs leaves each project as it was: the mesh, the undo history, the stroke
in progress or the tool you had open, the camera, the mask, the selection.
Closing a tab asks before discarding unsaved work. Quitting with several
unsaved tabs asks about each of them.

The project file does not change. A tab is a session in the running app,
not a new section in `.oxb`.

## Why

Today the window holds one document. File > New, File > Open, a
double-click, a drop, and a path on the command line all replace it, after
asking when there are unsaved changes (`Editor::request` / `Pending`).
Comparing two sculpts, or keeping a reference open while working on
another file, means a second process. The Blender bridge already does that
by accident: a second send starts another OxyBrush window.

The owner wants to switch in the same window and come back to whatever was
going on in each project.

## What the user sees

### The tab bar

The bar sits across the top of the viewport, under the menu bar and above
the model. It is part of the window, so hiding the panels (Tab) or hiding
only the settings panel (Shift+Tab) leaves it up. It does not sit in the
bottom strip: that space is the Parts Bin when [RFC 0026](0026-parts-bin.md)
lands.

Each tab shows the file name, or **Untitled** when the project has never
been saved. The tooltip is the full path. Two tabs with the same file name
also show the parent folder. A tab with unsaved changes has the same dot
the window title already uses (`Editor::title` puts `•` in front of the
name). The window title keeps following the active tab, dot included.

A tab with a background job shows that job's label in the tooltip
("Voxel remesh…") and a small progress mark on the tab, including while
the tab is not the one you're looking at.

The bar has a **+** at the right for a new tab. A tab closes from its
close button or a middle-click on it. Dragging a tab along the bar
reorders it. Dragging it off the bar does nothing in this design: it snaps
back. A separate window is later work, below.

When the tabs no longer fit, they shrink until the name ellipsizes, and
the ones that still don't fit are listed in a menu at the right end of
the bar. The active tab stays on the bar.

### Opening

An **empty** tab is an untouched starter: no path, no unsaved change, no
background job, and no tool waiting for Accept (a tube, an extract
preview, a curve, Poly Edit, retopology, a deform drag, a plugin preview,
the "edit this stroke as a curve?" question). The sphere you get at
startup is empty until you sculpt, rename a piece, or pick another starter.

| How the file arrives | What happens |
|---|---|
| File > New (sphere, cube, plane, human) | If the active tab is empty, that tab becomes the new starter. Otherwise a new tab opens with it. Nothing already open is discarded. |
| File > Open, a recent file, the Home window's Open | A project (`.oxb`) or a link manifest opens in a new tab, or in the active tab when that tab is empty. A mesh file still imports into the **active** tab, as new pieces, which is what Open does with OBJ, STL, PLY and glTF today. An `.oxyaddon` still installs into the shared plugin list. |
| The path is already open | Switch to that tab. Don't load a second copy. If the tab you were on was empty, close it, so opening doesn't leave a blank tab behind. |
| Double-click, Open With, drop on the window | Same rules as Open. On macOS, winit already delivers this to the running app as `DroppedFile`. On Windows and Linux a double-click still starts a new process; bringing that into the running window is the same later work as a second window. |
| Command line | One path works as it does now: that project is a tab (the empty startup tab is reused). Several project paths (`oxybrush a.oxb b.oxb`) open one tab each. A mesh path imports into the first tab. `--link` opens its manifest as a tab. `--screenshot` and `--demo` stay a single tab and do not restore a previous session, so automated runs keep opening one document. |
| The app is already running | A second process is still a second window. Only a file handed to this process (a macOS open, a drop) becomes a tab here. |

File > Import keeps adding pieces to the active tab.

### Closing, Revert, and quitting

File > Close project closes the **active** tab. The prompt is the one that
exists today (Save, Don't save, Cancel). The tab's close button is the
same action. Other tabs stay.

File > Revert reloads the active tab from its file and keeps that tab's
camera, which is what Revert does now. It asks first. It does not touch
other tabs. An untitled tab has nothing to revert to, so the command stays
disabled there.

Closing the **last** tab does not leave a window with no project. It opens
a fresh untitled tab and shows the Home window (the welcome window,
`show_welcome`, titled "Welcome to OxyBrush"), which is what Close project
does today when it was the only document.

Closing the **window**, or Quit, is still quit (`CloseRequested` already
quits; it does not mean "close this tab"). If more than one tab is
unsaved, one dialog lists them by name. Each row can be saved or skipped,
with Save all, Don't save, and Cancel. Cancel returns to the window with
every tab still open. A tab whose job holds subdivision levels waits for
that job before it saves, as Save does today.

### The Home window

The Home window stays a window of the app, not a tab of its own. It shows
at startup when the preference says so, and it closes when a file opens or
when you dismiss it, including when the file opened into a new tab. It
does not pop back up every time you switch to an untitled tab. Help >
Welcome screen still opens it. Its recent-file buttons follow the Open
rules above.

### Keyboard

The keymap treats the Command key on macOS as Ctrl, the same way Save and
Undo already do. Labels below say Ctrl; on a Mac that is Cmd.

Checked against the keymap table (`COMMANDS` in
`crates/oxy-app/src/keymap.rs`) and the shortcut labels the menus and
tooltips show (`Cmd::shortcut` in `crates/oxy-app/src/tips.rs`). The
shortcut window is built from the keymap (`shortcut_rows`), so both have
to change together. AGENTS.md still calls that list `SHORTCUTS` in
`ui.rs`; the list itself lives in these two places.

| Chord | What it does today | Proposed |
|---|---|---|
| Ctrl+T | Free. T alone is the transform gizmo. | New tab. |
| Ctrl+W | **Group from mask** (or the visible faces when nothing is masked). Shipped on purpose in keymap version 2. | **Conflict.** This RFC gives Close tab the chord people expect, and moves Group from mask to Ctrl+Shift+W. Shift+W stays the wireframe overlay; Ctrl+Shift+W is free. Left as an open question because it changes a shortcut testers have. |
| Ctrl+Tab, Ctrl+Shift+Tab | Free as commands. Tab alone shows or hides the panels. Shift+Tab shows or hides the settings panel. Tab is also the retopology "hold and drag" key: `note_held_key` marks Tab held even when Ctrl is down. | Next tab, previous tab. The chord must not toggle panels, and it must not set `tab_held`, so a retopology drag doesn't start. |
| Ctrl+1 … Ctrl+9 | Free. The bare digits 1–9 and 0 are brushes (and, in Poly Edit, Faces, Edges, Points and Auto on 1–4). Ctrl+0 frames the active piece. | Switch to that tab, counting from the left. Past the last tab, nothing happens. Ctrl+0 stays Frame. |
| Ctrl+Shift+T | Free. | Reopen the last closed tab, when it had a path. An untitled tab that was discarded is not reopened. |
| Ctrl+Page Down, Ctrl+Page Up | Free. | The same as Ctrl+Tab and Ctrl+Shift+Tab. Some desktops take Ctrl+Tab before the app sees it. |

## What stays with a tab

A tab is one document session. Switching away and back restores all of
this. None of it is written into `.oxb` unless it already was (the mesh,
the strokes, the tube, the curve, and so on). The camera pose is session
state today and stays session state: it is remembered with the tab while
the app runs, and a freshly opened file frames the model, as Open does now.

- The scene: pieces, mask, hidden faces, face groups, vertex color, UVs,
  per-piece materials, surface instances, subdivision levels.
- That project's undo history, and its living strokes.
- The path, the saved revision, and whether it is modified (including
  alphas or piece names changed since the save).
- The camera pose, what orbit turns around, and orthographic view.
- The tool you were in and how it was set: brush, size, strength, Gravity,
  falloff, focal shift, spacing, stroke mode, symmetry, Raise/Carve,
  dynamic detail, the gizmo and its unapplied session.
- The mask and which pieces are picked for the gizmo.
- An open tool and anything it has not committed: Poly Edit, the tube, a
  curve, retopology, the Deform window mid-drag, mask extract before
  Accept, a primitive being set up, a plugin preview waiting for Accept,
  the question after holding a stroke to turn it into a curve.
- The OxyCharacter link (`Link`) or the Blender bridge manifest
  (`Bridge`) for this project, if it has one.
- This project's alphas (the `ALPH` section): captured and loaded alphas
  belong to the tab that made them. Closing the tab drops them. This
  removes the current carry-over, where alphas stay in the list after
  Close project and get saved into the next file.
- The import scale chosen for this project, so later imports into it keep
  matching.
- That tab's background job, and that tab's autosave.

## What is shared

One window, one set of preferences, one GPU device. Changing any of these
is visible on every tab. They are not undo steps of a project.

- Theme, accent, corners, interface scale, layout, where the shelf and
  the panels sit, the keymap, brush order, units, the pressure curve.
- **Remember brushes.** A new tab starts from the remembered settings. The
  settings file stores the active tab's brush settings when it saves, as
  it stores the one document's settings today. Changing the size on one
  tab does not change the size on another.
- Focal length and the viewport lights. Both already live in the settings
  file, not in the project. Every tab looks through the same lens and the
  same lights. Per-piece materials stay in the project.
- Wireframe, face-group colors, the floor grid, and whether panels are
  shown. Those are the window.
- The brush preset library, custom matcaps, and the Parts Bin
  ([RFC 0026](0026-parts-bin.md)). The bin is files on disk. Its strip is
  the same strip on every tab. A drop sculpts the **active** tab only, as
  one undo step there. Saving a part back to the bin writes the library
  file, which no project's undo stack owns.
- Installed plugins and their remembered panel values (`plugin_params`).
  A preview a plugin is showing belongs to the tab, above.
- Recent files, window size and position, the updater, and whether the
  local agent is on.
- The Home window, Preferences, the Theme window, the shortcut list, and
  the guide.

Built-in alphas are shared constants. There is no separate user alpha
library on disk today (an alpha leaves the project only as an exported
PNG). A shared library is out of scope.

## Switching while something is in progress

Three cases, matching behaviour the app already has.

**A pointer drag** (a stroke, orbit, pan, a gizmo drag, a Poly Edit drag,
a selection). Switching ends it with `pointer_up`, the same call the app
makes when the window loses focus. A stroke you had started is committed
as a normal stroke on that tab, then the other tab appears. The switch
does not cancel it and does not leave the button "down" on a tab you
can't see.

**A tool waiting for Accept or Cancel** stays as it is, on the tab you
left. Coming back shows the tube, the extract, the curve, the unaccepted
plugin preview, or the hold-to-curve question, untouched. Switching does
not answer the question for you.

**A background job** keeps running on the tab that started it. The result
is applied to that tab even if another tab is active, and that tab's
progress mark shows it the whole time. Save on that tab still waits when
the job holds its subdivision levels.

Only one heavy job runs in the process at a time, which is the rule
today (`Editor` holds `Option<Job>`, and `start_job` refuses a second).
Each tab remembers its own job, so a finished job cannot land on the
wrong project. If you start a remesh on the tab you're looking at while
another tab's remesh is running, the new one waits and the tab says so.
Sculpting stays the only real-time work. Whether two heavy jobs should
run together is an open question; the default here is one, so a background
remesh doesn't take the cores the brush needs.

Piece ids are process-unique (`new_piece_id`), but a job result is still
tagged with the document it belongs to. `replace_document` today drops
`job` without waiting, and a late result is ignored only because the
piece is gone. With several documents that has to be explicit: a result
for a tab you closed is thrown away, and it is never applied to whichever
tab is active.

## Memory and the GPU

A 1.3-million-triangle mesh is about 68 MB
([CHANGELOG](../../CHANGELOG.md), the mesh-memory reduction). Undo keeps
its own budget, 2 GiB per history (`History::default`), and drops the
oldest steps past that. Living strokes default to 256 MB per project.
Subdivision levels store the coarser levels' positions beside the active
mesh; they are not another full copy of the top level per step, but they
do add up. Four such projects, with undo and strokes near their budgets,
are several gigabytes. That is the cost of keeping them open, and it is
why the budgets below are caps rather than "keep everything forever".

**CPU.** Every open tab keeps its mesh, history and strokes in memory, up
to a shared undo cap of 4 GiB across all tabs (each tab still stops at
2 GiB). Past the shared cap, the least recently used inactive tab loses
the oldest undo steps first. The mesh stays. Unloading a whole tab to its
autosave and reading it back on switch is later, after the switch time
below has been measured. The status bar can say when the open projects
are large; it does not refuse to open another one in the first version.

**GPU.** There is one `ViewportRenderer`, and its mesh buffers are keyed
by piece id. Each frame `retain` drops buffers for pieces that are not in
the scene being drawn. An inactive tab's buffers are dropped that way, and
switching back rebuilds them through `sync` (a full create when the piece
is not already on the GPU). Pipelines, the matcap, and the shadow maps
stay; they are redrawn for whichever tab is active. The preview slot
(`u64::MAX`) belongs to the active tab, so an inactive tab's preview mesh
is rebuilt when you return.

Piece ids don't collide across tabs, so a later change could keep inactive
buffers instead of dropping them, with no new key. This RFC drops them, so
several large meshes don't all sit in VRAM.

**How long a switch takes.** A switch to a tab whose mesh is still in
memory does not rebuild the mesh. `Mesh::from_data` at 1.3M triangles is
231 ms and at 5.2M is 1.1 s (`performance_notes.md`); a subdivision-level
switch to 5.2M is about 0.7 s and already runs off the UI thread. None of
that is a tab switch. What a tab switch pays is the GPU upload. The vertex
is 36 bytes (`GpuVertex`: position, normal, mask, group color, paint), so
a 1.3M-triangle sphere (about 655k vertices) is about 24 MB of vertices
and about 16 MB of indices, plus line and face-color buffers. **That
upload has not been measured.** The bar for the phase that adds switching:
on the machine used for `shading_timing`, a resident 1.3M-triangle mesh
is on screen within 200 ms of the click, and the previous tab's picture
stays up until the new buffers are ready, so a slow upload doesn't freeze
the window. The phase reports the number it measured. If it misses 200 ms,
the phase still ships with that number written down; it does not grow a
second, more complicated cache in the same change. A tab that had been
unloaded (the later memory work) switches by loading, on the same
background path as Open.

Nothing in the tab bar runs on the per-dab or per-frame sculpt path.

## Plugins, agents and bridges

**Which tab.** A new action uses the active tab. That includes a menu, a
plugin command, an agent tool, Send back, and a Parts Bin drop.

**Switching away does not retarget it.** A call that has started keeps the
tab it started on, the same way a background job does. The host snapshot
a WebAssembly plugin takes, and the `RunResult` it returns, are applied
to that tab and to no other. A Python plugin is still one process at a
time ("Another plugin is already running"); its listener edits the tab
that was active when it started, labeled `Plugin: <name>:` as now. The
tab shows that name while the plugin runs.

The agent is one loopback listener for the process ([RFC 0003](0003-mcp.md)),
off unless the user turns it on. Mutating tools still wait while that tab
is mid-stroke or mid-job; read-only tools still answer. `open` focuses the
tab when the path is already open, and otherwise opens a tab. It does not
replace the other projects. `save` writes the tab the call is bound to.
A screenshot is the viewport, so it shows whichever tab is active when the
frame is taken. A `tab` argument on the tools can wait; the first version
has one implicit tab per call.

`oxybrush-cli mcp` stays a single scene in that process. Tabs are the
desktop window.

**Bridges.** `Link` and `Bridge` live on the document. Opening a link
manifest follows the Open rules and keeps any other tab. Send back writes
that tab's files. The menu acts on the active tab, so you switch to the
linked project to send it; the tab can show a small link mark. A second
send from Blender still starts a second OxyBrush process, until the
single-instance work below. This RFC does not change the add-on.

## Undo, and copying a piece to another tab

Undo and redo stay inside the active tab, including undo on the active
piece only. A stroke, a remesh, a gizmo, a plugin write and an agent edit
are history entries of the tab they changed. There is no undo step that
contains two tabs, and there is no shared history to redo across them.

The one deliberate exception is copy and paste of pieces, proposed for the
last phase:

- **Copy piece** puts the active piece (or the pieces picked with it) on
  an app clipboard. The source tab does not change and gains no undo step.
- **Paste piece** adds those pieces to the active tab as one undo step,
  the same kind of step Duplicate uses. The clipboard holds the mesh, the
  mask, face groups, hidden faces, vertex color, UVs, subdivision levels,
  and that piece's living strokes. The strokes' vertex indices still refer
  to the copy, so they stay editable. If they would not fit in the
  destination's stroke budget, the mesh pastes and the strokes are baked,
  and the status line says so.
- Paste does not carry the source's undo history, camera, or tool. The
  two tabs are not linked afterwards. Editing one does not edit the other.

There is no piece clipboard today (Duplicate and Mirror duplicate are the
ways to copy inside one project).

## Remembering open projects, and Recent files

**Recent files** stay what they are: one list in the settings file, most
recent first, shared by every tab, updated when you open, import or save.
Closing a tab does not remove the path. Choosing a recent file follows the
Open rules, so a file that is already a tab is switched to, not loaded
again.

**Session restore** is separate, and it is not a `.oxb` section. On a
clean quit the settings file remembers the paths that were open, in order,
and which tab was active. The next start opens them as tabs. A missing
file is skipped, with one line in the status bar. Untitled work is not in
that list; if it was unsaved it is the autosave recovery, below. A
preference, on by default, turns restore off. Starting with a path on the
command line, or by double-click so the process is launched with a file,
opens that file and does not also reopen the previous set.

**Autosave.** Today the process writes a single `autosave-<pid>.oxb`, and
recovery looks for another process's file. With tabs, each dirty tab
writes its own file (`autosave-<pid>-<n>.oxb`). Recovery on the next start
offers each of them, and each accepted file opens as a tab. A clean close
of a tab, or a clean quit, removes that tab's file. The recovery scan
ignores every file from the running process, not only one name. The
interval stays the one in Preferences.

Restore and recovery don't both open the same project. Recovery wins for
a path that has an autosave: you get the recovered tab, not a second copy
from the session list.

## The project file

No `.oxb` change. The format is one scene per file: `META` (app version
and the active piece) and the piece sections (`PIEC`, and the optional
sections that follow it, including `STRK`, `STRI`, `ALPH`, `MRES`,
`MRCC`, `INST`, and the others in `crates/oxy-doc/src/format.rs`). There
is no list of open files, no camera, and no tab order. Older and newer
versions read a project saved while tabs exist exactly as they read it
today. Session restore and the recent list live in the settings file,
which is already where recent files live.

The camera, the open tool and the brush settings are session state. They
come back when you switch tabs during a run. They are not in the file
when you open it tomorrow, except what session restore can do from the
path alone (the file is reopened and framed, as Open frames it now).
Storing the camera in the settings session list, so restore puts the view
back, is an open question; it would still not be a `.oxb` field.

## How the code gets there

`App` already owns the window, the GPU context, egui, dialogs, the agent
listener and the settings files. `Editor` owns the one scene and almost
everything else (`crates/oxy-app/src/editor.rs`). The split is inside
`Editor`, so the UI can keep taking `&mut Editor`. A second `Editor` per
tab would duplicate the keymap, the plugins and the presets.

`Document` holds the per-tab fields in the list above. `Editor` holds
`documents`, the active index, and the shared fields. Phase 1 is that
move with exactly one document, and every existing test still passes.

### Phases

Each phase is a change that ships on its own. The app is usable at the
end of each.

1. **One document, no visible change.** Move the per-tab fields into
   `Document`. `self.scene` becomes the active document's scene. One
   document only. Autosave, jobs, undo and the window title behave as
   they do now. This is the large move, and it has no new UI, so a
   regression is a failed existing test rather than a new interaction.
2. **Several documents, the bar, and one autosave file per dirty
   tab.** New, open, close, switch, the unsaved dot, the title,
   empty-tab reuse, and "already open" focuses the tab. GPU `retain`
   sees only the active document. A job result carries its document id.
   One heavy job at a time. The Home window follows the rules above.
   Each dirty tab writes `autosave-<pid>-<n>.oxb`, a clean close removes
   that file, and the next start's recovery offer opens one tab per
   file. A crash must not keep only the tab you happened to be looking
   at. The recovery scan ignores every file from the running process.
3. **Shortcuts, reorder, overflow, the quit dialog, Revert per tab,
   drag-drop and double-click, several paths on the command line.**
   Update the keymap and the shortcut labels. A headless screenshot of
   the bar.
4. **Session restore, and Recent files focusing an open tab.** The
   per-tab autosave files already exist from phase 2.
5. **Agents, plugins and bridges latch the tab they started on. Copy
   piece and Paste piece.**

Later, not in these phases: dragging a tab out into its own window;
a single instance so a Windows or Linux double-click, and a second
Blender send, open a tab in the running app; unloading an inactive tab's
mesh past a memory budget; an explicit tab argument on agent tools.

### Performance budgets

- No work added on the per-dab path. `dab_timing` on a 1.3M-triangle
  mesh stays in the band it is in before phase 2 (small dab about 0.6 ms,
  large dab about 2–2.4 ms, from the README's 4-core run).
- Switching to a resident 1.3M-triangle project: the previous frame stays
  up, and the new one is up within 200 ms, measured in the phase 2 PR.
  Not measured yet. The figure above is the bar, not a result.
- Applying a finished job costs what it costs today. Doing it while that
  tab is inactive does not upload that mesh to the GPU.
- The tab bar is egui layout, once per frame, not once per dab.

### Tests

In `oxy-app`, beside the editor tests:

- Two documents. A stroke and an undo on one leave the other's mesh,
  history revision and camera alone.
- Switching back restores the brush, the camera, and an unaccepted
  extract or tube.
- A stroke that is down at the moment of the switch is committed on the
  tab that was active, and is one undo step there.
- A job started on tab A, after a switch to tab B, applies on A. A job
  whose tab was closed does not apply anywhere.
- Opening a path that is already a tab focuses it. An empty tab is
  reused and, when a different path is focused, not left behind.
- Close asks only about that tab. Quit with two unsaved tabs lists both.
  Revert reloads one tab and keeps its camera.
- Ctrl+T, Ctrl+Tab and Ctrl+1 do not run the gizmo, the panel toggle,
  `tab_held`, or brush 1. Ctrl+W does whatever the maintainer decides
  below, and the keymap test covers that chord.
- Copy and paste: the destination gains one undo step and the piece; the
  source's history revision is unchanged. Undo on the destination removes
  the pasted piece and does not change the source.
- An agent `open` adds or focuses a tab and leaves the other document's
  revision alone. A plugin result recorded against tab A is refused if
  applied while B is active.
- Autosave writes one file per dirty tab; a clean close removes it;
  recovery opens one tab per file.
- Existing `.oxb` round-trip tests still pass, and a save made with
  several tabs open is byte-for-byte the same format (no new tag).

A headless screenshot in phase 3 shows the bar with two titles, one of
them unsaved. It does not replace the tests above.

## Risks

- `Editor` is the type most of `oxy-app` takes. Phase 1 will touch a lot
  of lines for a move that should not change behaviour. Landing it alone,
  before any tab UI, is what makes that reviewable.
- A late job, a plugin result or an agent call applied to "the" editor
  after a switch will edit the wrong project. The document id has to be
  checked at the apply site, not only when the work starts.
- Autosave is one file per process today. Phase 2 writes one file per
  dirty tab before the tab bar is something people rely on. Shipping
  the bar without that would mean a crash keeps only the tab you were
  looking at.
- The undo cap is 2 GiB **per history**. Several tabs each fill that.
  The shared 4 GiB cap has to exist in the same phase as the second
  document, or the first real use will look like a leak.
- `tab_held` follows the Tab key with any modifiers. Ctrl+Tab has to
  opt out, or retopology starts a drag when you switch tabs.
- Taking Ctrl+W changes a shortcut that has been Group from mask since
  keymap version 2. The keymap file migrates old bindings; this one needs
  the same care, so a saved keymap doesn't silently close a tab.
- Some window managers eat Ctrl+Tab. The Page Up / Page Down aliases are
  there for that, and they should be tested on one Linux desktop before
  the shortcut is called done. This environment can't promise that.
- macOS document registration (`register_document_types`) tells Finder
  that `.oxb` belongs to the app. It is not a document object per file.
  The window title follows the active tab. Setting the title-bar proxy
  icon to that file is a small extra and is easy to get wrong without a
  Mac; it can wait, and the first version should not claim the proxy icon
  works until someone tries it.
- Lavapipe (the headless Vulkan used for screenshots) is a poor stand-in
  for the 200 ms upload bar. The phase 2 number has to say which GPU it
  was measured on.

## Alternatives

**One process per project.** This is what a second OxyBrush window is
today. A crash takes one project with it, and the projects share nothing
except files on disk (presets, the Parts Bin). Switching is an operating
system window switch. Copying a piece is export and import. It does not
need this refactor, and it does not give you the tab you were in the
middle of inside the same window. The owner asked for tabs.

**Several windows in one process, and no tabs.** The same `Document`
split, spent first on windowing. A second surface can share the GPU
device, and the code for that is larger than a tab bar. It is the right
shape for dragging a tab out later. It is a poor first step, because the
daily action is switching, not arranging windows.

**Tabs in one process.** This RFC. The `Document` type is what the
separate window would reuse, so choosing tabs now does not block that
later.

## Open questions

The proposal is in the sections above. These are the choices that change
a shortcut, a file, or what an agent does.

1. **Ctrl+W.** Close tab takes it, and Group from mask moves to
   Ctrl+Shift+W. Is that acceptable, with a keymap migration so an old
   saved binding doesn't close a tab?
2. **Brush settings per tab.** Size, brush and symmetry stay with the
   tab. Remember brushes stores the active tab as the default for a new
   one. Should a change on one tab ever push to the others?
3. **Focal length and lights** stay shared preferences, as they are in
   the settings file today. Should a project keep its own?
4. **A stroke or gizmo drag in progress** is committed with `pointer_up`
   when you switch, the same as the window losing focus. Is committing
   right, or should the switch wait until the button is up?
5. **One heavy job at a time** for the whole process, and it keeps
   running on a tab you have left. Should a second tab's remesh run in
   parallel instead?
6. **Session restore** on by default, paths only, in the settings file,
   and skipped when the program was started with a file. Should the
   camera pose be stored there too?
7. **Agent `open`** focuses or adds a tab and leaves the other projects
   open. Should it keep today's meaning and replace the active tab?
8. **Copy and paste of pieces** in phase 5, carrying living strokes and
   subdivision levels, baked if the stroke budget refuses them. Is mesh
   alone enough for the first paste?
9. **Inactive tabs drop GPU buffers** and keep the CPU mesh, with a
   shared 4 GiB undo cap. Is 4 GiB the right cap, and should the first
   version unload a whole tab instead?
10. **The 200 ms switch bar** for a resident 1.3M-triangle mesh, with the
    previous frame left up, and the measured number reported rather than
    a second cache built in the same change. Is 200 ms the right bar?
11. **The quit dialog** lists every unsaved tab, each saveable or
    skippable, plus Save all, Don't save and Cancel. Is one prompt after
    another preferable?
12. **Dragging a tab out** into its own window waits until tabs
    themselves work. Confirm that it is later.
