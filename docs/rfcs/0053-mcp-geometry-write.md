# RFC 0053 — MCP geometry write (colors + displacement)

**Status:** Dense and sparse writes, grouped undo, geometry event origins, viewport images and light data implemented
**Issue:** [#555](https://github.com/crisprintsstuff/OxyBrush/issues/555)

## Summary

Add an MCP tool `set_geometry` that applies **positions**, **vertex
colors**, and/or **mask** from an `OXYG` file onto an existing piece. One
undo step, labeled `Agent:` / `Plugin: <name>:`. Same binary format as
`get_geometry` ([RFC 0007](0007-plugin-api-read-access.md) / [RFC 0003](0003-mcp.md)).

## Motivation

In-app WASM plugins already have `write_positions` / `write_colors` (API 2).
Python and other MCP clients could only **read** geometry. Artists asked for
polypaint and mesh displacement from plugins and agents without going through
strokes. Plugins can now supply the rendered pixels from their own renderer as a temporary image in the desktop viewport.

## Design

| Input | Behavior |
|---|---|
| `path` | Local `OXYG` file (same layout as `get_geometry`) |
| `id` / `index` / `name` | Target piece; omit for active |
| `topology_version` | Optional; refuse if the piece's triangles changed |

Writable sections: `positions`, `colors`, `mask`. **Ignored:** `normals`,
`indices`, `groups`, `hidden` (topology stays with remesh / WASM
`write_mesh`). Vertex and triangle counts in the header must match the live
piece. Colors clamp to 0..=1; mask to 0..=1. Non-finite values refuse.

Undo mirrors WASM vertex edits: `begin_with` (snapshot color when painting),
`touch` every chunk, apply, `update_chunks` + `tighten_bounds`, `commit`.
Desktop reveals vertex color when colors are written.

Python SDK: `Session.set_geometry(path=…)` or
`set_geometry(arrays={…})` (builds a temp OXYG from `get_geometry` counts).

## Sparse writes

`patch_geometry` takes a required `topology_version` from
`get_geometry` and a `patch` object containing `indices` and at
least one of `positions`, `colors` or `mask`. Arrays use one
triple or scalar per index in the specified order. Indices must be
unique and in range. Validate the complete request first and take
a snapshot of each modified chunk in one undo step. No-op values do
not create history. Python exposes `Session.patch_geometry`.

## Desktop viewport images

`get_viewport_state` reports the physical pixel dimensions, camera
matrix in column-major order, and a view token for the current scene.
Render off the UI thread, then call `set_viewport_image(path, view_token)`
with a local file containing exactly width × height × 4 bytes of
straight-alpha RGBA8 (sRGB channels, rows from top to bottom, no header).
The file limit is 64 MiB. The app loads it on a worker and checks the
token again before showing it. The image disappears when the camera,
viewport dimensions, visible mesh state, or project changes.
`clear_viewport_image` also removes it. These methods are exposed
through the Python SDK and local MCP in single-viewport mode.

## Grouped Undo and change origin

Pass the same optional `undo_group` to consecutive `patch_geometry`
and `set_geometry` calls from one listener, targeting one piece with
the same topology. The group keeps the earliest snapshot of each
changed chunk, including its colors, and Redo restores the final
state. An unrelated history edit, Undo, Redo, or a write to another
piece starts a new group. Omitting the field makes a separate undo
step. Reuse the group name only within a sequence you want to undo
together.

A `changes` reply includes the listener's opaque `session_origin`.
Geometry events from those tools include `origin`. Compare the two
to filter out your own writes. Artist edits and events without a
known origin omit `origin`. The desktop app gives each Python run
its own listener and ID. External clients sharing one listener also
share its ID. Headless sessions provide their own ID.

## Viewport lights

Desktop `scene_state` includes `lighting` with the enabled lights,
linear color and intensity, ambient rig, exposure, surface settings,
and shadow settings. Positions, aim points, sun directions, and area
diameters are resolved in world space. Distance falloff uses the
reported `scene_size`. Headless sessions have no viewport rig.

## Follow-up

- Expose viewport image writes to in-app WASM.
- Topology replacement over MCP.

## Acceptance

- Round-trip `get_geometry` → edit colors/positions → `set_geometry` undoes
- Stale `topology_version` and mismatched counts error without changing the mesh
- WASM plugin Displace / Color Ramp behavior is unchanged
