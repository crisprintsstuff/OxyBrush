# Plugin host API reference

An in-app plugin imports these functions from the module **`oxybrush`**.
This is API version 4, which adds the rows marked *(4)*, along with *(3)*
and *(2)* from earlier versions. Older plugins keep working unchanged.

## Conventions

- **Types:** every argument and result is `i32` or `f32`.
- **Pointers and lengths** are byte offsets and counts in the plugin's
  exported `memory`. The host reads and writes only inside it, and checks
  the bounds.
- **Layouts:**
  - positions, normals and colors are three little-endian `f32` per vertex
    (`x, y, z` or `r, g, b`);
  - triangles are three `u32` vertex indices, counter-clockwise seen from
    outside;
  - masks are one `f32` per vertex;
  - face groups are one `u32` per triangle;
  - hidden flags are one `u8` per triangle.
- **Results:** a negative result is an [error code](#error-codes).
  - Reads return how many items they wrote, at most `max`.
  - Writes return 0.
- **Piece indices** run from 0 to `piece_count() - 1`, in the order of the
  Pieces panel. `active_piece()` is the one selected.
- **The scene is a copy, taken when the command starts.** Writes don't
  change what later reads return. All of a command's writes are applied
  together when it returns.

## Exports the plugin provides

| Export | |
|---|---|
| `memory` | required: the plugin's linear memory |
| `run(command: i32)` | required: called with the index of the command clicked |
| `overlay()` | optional: called each frame while the plugin's panel is open, to draw lines (needs `overlay`) |

## Functions

### Always available

| Function | |
|---|---|
| `log(ptr, len)` | UTF-8 text (up to 4096 bytes) for the status bar and the log file; the last line logged by a command is shown |
| `param(index) -> f32` | the current value of param `index` (see the [manifest](manifest.md#params)); 0 for an unknown index |
| `param_color(index, ptr) -> i32` *(3)* | a color param's value as three `f32` (sRGB 0..=1) at `ptr` |
| `progress(fraction: f32) -> i32` *(3)* | report progress, 0..=1, shown as a bar; returns **1 once the user has clicked Stop**, else 0 |
| `piece_count() -> i32` | pieces in the scene |
| `active_piece() -> i32` | index of the active piece |

### Reading meshes: `read_mesh`

| Function | |
|---|---|
| `vertex_count(piece) -> i32` | |
| `triangle_count(piece) -> i32` | |
| `read_positions(piece, ptr, max) -> i32` | up to `max` positions |
| `read_triangles(piece, ptr, max) -> i32` | up to `max` triangles |
| `read_mask(piece, ptr, max) -> i32` | up to `max` mask values; 1 = fully protected |
| `read_normals(piece, ptr, max) -> i32` *(2)* | up to `max` unit vertex normals |
| `read_colors(piece, ptr, max) -> i32` *(2)* | up to `max` vertex colors, sRGB 0..=1, white where unpainted |
| `read_groups(piece, ptr, max) -> i32` *(3)* | up to `max` face groups |
| `read_hidden(piece, ptr, max) -> i32` *(3)* | up to `max` hidden flags, 1 = hidden |

### Changing meshes: `edit_mesh`

Each write replaces that attribute for the **whole piece**. `count` must
be the piece's vertex count, or its triangle count for face groups and
hidden flags. If a command writes the same attribute twice, the last write
wins.

| Function | |
|---|---|
| `write_positions(piece, ptr, count) -> i32` | new positions for every vertex; non-finite values are refused |
| `write_mask(piece, ptr, count) -> i32` *(2)* | a mask value for every vertex, clamped to 0..=1 |
| `write_colors(piece, ptr, count) -> i32` *(2)* | a color for every vertex, clamped to 0..=1 |
| `write_groups(piece, ptr, count) -> i32` *(3)* | a face group for every triangle |
| `write_hidden(piece, ptr, count) -> i32` *(3)* | a hidden flag for every triangle; nonzero = hidden |

### Changing topology: `edit_topology` *(3)*

| Function | |
|---|---|
| `write_mesh(piece, pos_ptr, vcount, tri_ptr, tcount) -> i32` | replace the piece's vertices and triangles |

- **Limits:** 3 to 4,000,000 vertices, 1 to 4,000,000 triangles, finite
  positions, and indices below `vcount`.
- **Later writes use the new mesh.** After `write_mesh`, this command's
  `write_mask` and `write_colors` take `vcount`, and `write_groups` and
  `write_hidden` take `tcount`. `write_positions` returns `ERR_CONTEXT`,
  since you just gave the positions.
- **Earlier writes are dropped.** Anything written to the piece before
  `write_mesh` was for the old mesh, so it's discarded.
- **What isn't written starts fresh:** unmasked, white, face group 0, all
  visible. To keep attributes, read them first and write them back after;
  the bundled Flip Faces, Fill Holes and Simplify Mesh plugins do this.
- **Subdivision levels** of the piece are removed. Undo restores them, and
  the old mesh.

### New pieces: `add_pieces` *(2)*

| Function | |
|---|---|
| `add_piece(name_ptr, name_len, pos_ptr, vcount, tri_ptr, tcount) -> i32` | a new piece, with the same limits as `write_mesh`; returns its index among this command's new pieces |

All of a command's new pieces become one undo step, and the last one
becomes active.

### Scene: `read_scene`

| Function | |
|---|---|
| `scene_state(ptr, cap) -> i32` | the scene as JSON: pieces, their versions, the camera; writes up to `cap` bytes and returns the full length (call with `cap` 0 to size a buffer) |

### Living strokes: `read_scene`, `edit_strokes` *(4)*

| Function | Needs | |
|---|---|---|
| `strokes_state(ptr, cap) -> i32` | `read_scene` | the active piece's [living strokes](../rfcs/0017-living-strokes.md) and stroke groups as JSON; writes up to `cap` bytes and returns the full length |
| `stroke_command(ptr, len) -> i32` | `edit_strokes` | queue one Strokes window action, as UTF-8 JSON; returns its index, or `ERR_COUNT` if it isn't a JSON object with an `"op"` (at most 1,000 a command) |

`strokes_state` returns the same JSON as the agent's `list_strokes`:

```json
{"enabled": true, "piece": {"id": 1, "name": "Head"},
 "strokes": [{"id": 3, "brush": "draw", "made": "14:02", "points": 41,
              "strength": 1.0, "group": 1}],
 "groups": [{"id": 1, "name": "Wrinkles", "strength": 0.6, "visible": true,
             "strokes": [3]}],
 "solo": null, "memory_mb": 1.2, "budget_mb": 256}
```

`enabled` is false (and the lists empty) while Living strokes is off in
Settings. Actions name strokes and groups by those ids:

```json
{"op": "set_strength", "strokes": [3, 4], "strength": 0.5}
{"op": "delete", "strokes": [3]}
{"op": "solo", "strokes": [3]}            {"op": "show_all"}
{"op": "keep_unmasked", "strokes": [3]}
{"op": "group", "strokes": [3, 4], "name": "Wrinkles"}
{"op": "add_to_group", "strokes": [5], "group": 1}
{"op": "remove_from_group", "strokes": [5]}
{"op": "group_strength", "group": 1, "strength": 0.6}
{"op": "group_visible", "group": 1, "visible": false}
{"op": "rename_group", "group": 1, "name": "Scales"}
{"op": "merge_groups", "from": 2, "into": 1}
{"op": "ungroup", "group": 1}
{"op": "delete_group", "group": 1}
{"op": "bake"}
{"op": "mirror", "strokes": [3], "axis": "x"}
{"op": "split_by_side", "strokes": [3], "axis": "x"}
```

`mirror` adds the strokes' mirror image across the piece's symmetry plane
as new strokes; `split_by_side` makes each stroke that changed both sides
one stroke per side. `axis` is `"x"` (default), `"y"` or `"z"`. Strengths run from 0 to 1.5 (150%). Actions are applied after the command's
mesh writes, in the order queued, each as its own undo step. One that names
an unknown id or changes nothing is skipped, and the status bar shows the
first reason.

### Overlay: `overlay`

| Function | |
|---|---|
| `draw_line(x1, y1, z1, x2, y2, z2, rgba) -> i32` | during `overlay()` only: a line in world space, color `0xRRGGBBAA`; up to 20,000 lines a frame |

Mesh functions return `ERR_CONTEXT` during `overlay()`, and `draw_line`
returns it outside.

## Error codes

| Code | Name | Means |
|---|---|---|
| -1 | `ERR_DENIED` | the manifest doesn't declare the capability |
| -2 | `ERR_PIECE` | no piece with that index |
| -3 | `ERR_MEMORY` | a pointer or length outside the plugin's memory |
| -4 | `ERR_COUNT` | a count that doesn't match the piece, or values that aren't allowed (non-finite, indices out of range, over the limits) |
| -5 | `ERR_CONTEXT` | not allowed here: mesh functions in `overlay()`, `draw_line` outside it, `write_positions` after `write_mesh` |

## How edits are applied

When `run` returns, OxyBrush applies the command's writes:
- each piece's edits as **one undo step**, labelled
  `Plugin: <name> — <command>`;
- its new pieces as one more step.

If the piece changed while the command ran, its edits are skipped and the
status bar says so. If the user clicked **Stop**, nothing is applied, even
if the plugin finished anyway.

## Limits

| | |
|---|---|
| Fuel per command | about 1,000,000,000 instructions, then the command is stopped |
| Fuel per overlay frame | about 20,000,000 instructions |
| Memory | 512 MB |
| Imports | only the functions above; a module importing anything else doesn't load |
