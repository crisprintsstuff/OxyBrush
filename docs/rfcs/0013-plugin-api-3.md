# RFC 0013: Plugin API 3 — topology, live preview, richer panels

- **Status:** proposed for the maintainer; implemented in the same change for review
- **Builds on:** [RFC 0011](0011-wasm-plugins.md) (in-app WebAssembly
  plugins, API 1 and 2) and [RFC 0007](0007-plugin-api-read-access.md)
  (the scene model plugins see)

## Problem

In-app plugins can move vertices, set mask and color, and add pieces.
They can't do several things plugins elsewhere routinely do:

- **Change face groups, hidden faces or the triangles of a piece.** That
  rules out cleanup tools, "group by angle", flipping faces, filling holes
  and custom remeshers.
- **Show a result before it's applied.** A plugin only runs when its
  button is clicked. Tuning Displace means click, look, undo, move the
  slider, click again.
- **Offer anything but sliders.** There are no on/off switches, choices,
  colors or explanatory text.
- **Remember settings.** Slider values reset every session.
- **Report on long work.** A long command can't show progress or be
  stopped.

## Decision

**API 3**, additive like API 2: plugins written for API 1 and 2 run
unchanged, and OxyBrush runs all three.

### Faces and topology

| Function | Capability | |
|---|---|---|
| `read_groups(piece, ptr, max) -> i32` | `read_mesh` | a `u32` face group per triangle |
| `read_hidden(piece, ptr, max) -> i32` | `read_mesh` | a `u8` per triangle, 1 = hidden |
| `write_groups(piece, ptr, count) -> i32` | `edit_mesh` | a `u32` face group per triangle (`count` = triangle count) |
| `write_hidden(piece, ptr, count) -> i32` | `edit_mesh` | a `u8` per triangle, nonzero = hidden |
| `write_mesh(piece, pos_ptr, vcount, tri_ptr, tcount) -> i32` | **`edit_topology`** (new) | replace the piece's vertices and triangles |

- After `write_mesh`, the piece's other writes in that command refer to
  the new mesh: `write_mask` and `write_colors` take the new vertex
  count, and `write_groups` and `write_hidden` take the new triangle count.
  `write_positions` is refused (`ERR_CONTEXT`), since `write_mesh` already
  gave the positions.
- Anything not written for the new mesh starts fresh: unmasked, white,
  face group 0, all visible.
- Limits are the same as `add_piece`: at least 3 vertices and 1 triangle,
  at most 4 million of each, finite positions, indices in range.

`edit_topology` is its own capability because it can throw away detail
(and subdivision levels) that `edit_mesh` can't. The Plugins settings list
says "changes topology".

**Undo:** all of one command's edits to a piece are still **one undo
step**:
- a topology change is recorded like a remesh (the old mesh and levels
  are kept, so undo restores them);
- face edits together with vertex edits are recorded as one combined step
  (a new `History::commit_with_faces`);
- face edits alone are an ordinary face step.

### Live preview

A command can be marked `"preview": true` in `plugin.json`. Clicking it,
in the panel, the Plugins menu or the command palette, opens the plugin's
panel and starts a preview instead of applying it. While the panel is
open:
- the command runs, on a worker thread as before, once at the start and
  again whenever a param changes;
- the result is shown in place of the active piece, the way an open
  curve stroke is;
- the panel shows **Accept** and **Cancel**. **Accept** applies exactly the
  shown result (it isn't run again), as the usual undo step. **Cancel**,
  closing the panel, or editing the piece some other way drops it.

Only the command's edits to the **active piece** are previewed. New pieces
and edits to other pieces appear on Accept. A preview run that's
overtaken by a newer param change is discarded. The newest values always
win, and at most one run is pending.

### Richer panels

A param can have a `"type"`:

| Type | Shows | `param(index)` returns |
|---|---|---|
| `"slider"` (default) | a slider; `"step": 1` makes it whole numbers | the value |
| `"toggle"` | a checkbox | 0 or 1 |
| `"choice"` with `"options": ["X", "Y", "Z"]` | a drop-down | the index of the chosen option |
| `"color"` with `"default": [r, g, b]` | a color button | 0; read it with `param_color(index, ptr)`, which writes three `f32` (sRGB 0..=1) |
| `"label"` with `"text"` | text in the panel | 0 |

A param may also carry `"help"`: a tooltip. Unknown types are refused at
load, like unknown capabilities.

### Remembered settings

Param values are saved in the settings file per plugin (by its folder
name) and param id, and restored when plugins load. Values outside a
param's range, or for params that no longer exist, are ignored. Each panel
has a **Reset** that goes back to the manifest defaults.

### Progress and cancel

| Function | Capability | |
|---|---|---|
| `progress(fraction: f32) -> i32` | — | report 0..=1 progress; returns 1 once the user has asked to stop |

While a command runs, its panel and the Plugins menu show a progress bar
(when the plugin reports one) and a **Stop** button.
- **Stop** asks the plugin to finish early through `progress`.
- Whatever the plugin does, a stopped command's edits are **discarded**.
  Nothing it did is applied.
- A plugin that never calls `progress` still stops at its fuel limit, as
  before.

## Examples shipped with it

- **Group by Height** (`read_mesh`, `edit_mesh`) splits a piece into bands
  of face groups along an axis. It uses a `choice` (axis), a whole-number
  slider (bands), live preview and `progress`.
- **Flip Faces** (`edit_topology`) turns a piece inside out with
  `write_mesh`: a minimal topology example.
- **Displace** gets live preview. Bundled plugins are copied into the
  plugins folder only when they're missing, so people who already have
  Displace keep their old copy until they delete its folder.

## Not in this version

- Partial writes (only some vertices) for large meshes, selection and
  cursor context, keyboard shortcuts for plugin commands, and brush
  plugins. See the plugin roadmap discussion.
- A Rust SDK. It needs `unsafe` imports, so it belongs in its own
  repository.
- Previewing new pieces or edits to pieces other than the active one.

## Documentation

A guide, a manifest reference and the full host API reference are in
[`docs/plugins/`](../plugins/README.md), with a WebAssembly-text example and
a Rust example that was built for `wasm32-unknown-unknown` and run against
the runtime. The same pages are published to the GitHub wiki.

## Verification

- `oxy-plugin` tests:
  - every new function denied without its capability;
  - `write_mesh` limits and the after-`write_mesh` counts;
  - `write_positions` refused after `write_mesh`;
  - new param types parsed, and bad ones refused;
  - `param_color`;
  - `progress` reporting and the cancel flag.
- Editor tests:
  - a face-group edit with a vertex edit is one undo step that undoes
    exactly;
  - a topology edit undoes to the old mesh and levels;
  - preview runs on param change, shows the active piece's result, and
    Accept applies it as one step; Cancel leaves the piece untouched;
  - param values survive a settings round trip and ignore bad values;
  - a stopped command applies nothing.
