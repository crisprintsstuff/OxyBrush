# RFC 0053 — MCP geometry write (colors + displacement)

**Status:** Accepted (Phase 1)
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
strokes. Path tracing, lights and viewport effects stay out of scope here.

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

## Out of scope (later)

- Partial / sparse vertex writes without a full array
- Topology replacement over MCP
- Plugin access to the sculpt viewport for path tracing, lights, effects

## Acceptance

- Round-trip `get_geometry` → edit colors/positions → `set_geometry` undoes
- Stale `topology_version` and mismatched counts error without changing the mesh
- WASM plugin Displace / Color Ramp behavior is unchanged
