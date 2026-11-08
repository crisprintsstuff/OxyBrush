# RFC 0007: Plugin API, phase 1: read access

- **Status:** proposed for the maintainer; implemented in the same change for review
- **Prompted by:** a contributor's writeup on adding viewport lights to a
  private build of OxyBrush. The lights had to be written into the core,
  because plugins can't reach the renderer, and the writeup proposed a
  phased plugin API. This RFC takes up its first phase. No code from that
  build was used.

## Problem

Plugins are separate programs (the Python SDK, or any MCP client) that
drive OxyBrush through the local MCP server ([RFC 0003](0003-mcp.md)). That
works for automation, but a plugin that keeps its own copy of the scene (an
external renderer, a turntable, an analysis tool) hits three walls:

1. **The camera and scene are text.** `scene_info` gives yaw, pitch and
   distance on one line; there's no field of view, target or matrices, and
   no reliable way to tell whether a piece changed.
2. **Changes can only be polled,** by re-reading that text.
3. **Geometry only comes out through export.** A file round trip in a
   format meant for other programs: the exporters drop vertex colors, and
   PLY merges pieces.

## Decision

Three read-only tools, in both the app and `oxybrush-cli mcp`, built from
one shared module (`oxy_mcp::state`) so both answer identically:

### `scene_state`

The document as JSON (`api_version: 1`). It includes:
- the document's path, modified flag and history revision;
- every piece: id, name, index, visibility, active, counts, bounds, painted,
  subdivision levels, symmetry centre;
- the visible bounds;
- the camera: eye, target, up, forward, distance, yaw and pitch, vertical
  field of view, orthographic flag and half-height, column-major view and
  projection matrices (wgpu convention, depth 0–1), viewport size and
  pixels per point.

Each piece also carries three numbers: `topology_version` (new triangles),
`edit_version` (positions, mask, color) and `face_version` (face groups,
hidden faces). If all three match what a client saw before, the piece is
unchanged. `edit_version` is a new counter on `Mesh`, bumped wherever the
mesh marks a chunk for re-upload. Every visible change already goes through
that, because the renderer depends on it.

### `changes`

A numbered log of changes, kept by diffing a small snapshot of the document
(per piece: id, name, visibility, the three versions; plus the active piece,
the camera and the document path or saved state).
- **Events:** `piece_added`, `piece_removed`, `geometry_changed` (with
  `topology_changed`), `faces_changed`, `visibility_changed`, `renamed`,
  `active_piece_changed`, `camera_changed`, `document_changed`.
- **Usage:** call without `since` to get a cursor; pass the returned cursor
  each time after that.
- **Waiting:** with `timeout_ms` (capped at 10 s) the call waits for the
  next change. In the app it waits on the listener thread, never the UI
  thread.
- **Merging:** repeated events of one kind are merged (dragging the camera
  is one `camera_changed`, not one per frame).
- **Limit:** the most recent 1024 are kept; an older cursor gets
  `truncated: true` and should re-read `scene_state`.

In the app the snapshot is taken once per frame while the agent server is
on (a few numbers per piece). In the CLI, only the client's own calls change
the document, so `changes` never waits.

**Why a cursor and a wait rather than pushed notifications:**
- The loopback transport is request/response over HTTP. Server push would
  need a second, long-lived stream, with its own state and its own failure
  modes.
- A cursor survives a client reconnecting, and makes it obvious when events
  were missed.
- It works unchanged over stdio and over the attach bridge.

### `get_geometry`

A piece's arrays, written to a binary file:
- **Fields:** positions, normals and indices by default (plus colors if the
  piece is painted); mask, face groups and hidden faces on request.
- **Format:** `OXYG`, little-endian. A 24-byte header (magic, format
  version, vertex and triangle counts, a bit set of the sections, reserved),
  then each section 4-byte aligned.
- **Reply:** the file's path and a `layout` with every section's offset,
  type, components and whether it's per vertex or per triangle, plus the
  piece's versions, so a client can tell which state it read.
- **Where the file goes:** OxyBrush picks the path (a per-process folder in
  the system temp directory), never the client. It writes atomically and
  keeps only the newest four files.
- **In the app:** the arrays are copied on the UI thread (a memory copy)
  and the file is written on a background thread.

**Why a file and not bytes in the reply:** MCP replies are JSON, so binary
would travel as base64. That's a third larger and slower to parse, and a
1.3M-triangle piece is about 30 MB. Plugins run on the same machine and can
read a file directly.

### Python SDK

`Session.scene_state()`, `changes()`, `follow_changes()` (a generator of
events) and `get_geometry()` (reads the file into `array.array` values).
The SDK stays pure Python with no dependencies.

## What this enables

An external or path-traced renderer can stay in sync with the document:
- copy the camera exactly;
- re-read a piece only when its versions change;
- get vertex colors and separate pieces;
- react to edits within a frame of them happening.

The contributor's offline path-tracer prototype, for example, had to guess
the camera from text and round-trip through GLB export.

## Not in this phase

Phase 2 (a later RFC): plugin panels drawn by the app, commands and
shortcuts, viewport overlays with hit-testing, and an image overlay.
Phase 3 (only if needed): plugin-supplied shading, extra render passes and
post effects. Separately, the exporters should carry vertex colors; that's
an exporter fix, not plugin API.

## Transport: MCP now, in-process later

MCP over loopback is a good fit for automation, AI agents and external
tools:
- any language works;
- a plugin is a separate process, so it can't crash or slow the app;
- the token and loopback-only rules from RFC 0003 cover it.

It is a poor fit for anything that must run every frame inside the app
(overlays, shading): every call crosses a process boundary as JSON.

The roadmap's WASM component plugin API is the in-process answer. Plugins
compiled to WebAssembly would run sandboxed inside OxyBrush, with only the
capabilities they declare. The read-access shapes defined here (scene
state, versions, change events, geometry layout) are meant to carry over to
that API as its first interface, so plugins see one model whichever way
they connect.

## Verification

- Unit tests (`oxy_mcp::state`):
  - the JSON shape;
  - change detection, including merging and truncation;
  - waiting across threads;
  - the geometry encoding read back field by field;
  - file pruning.
- A CLI-level test calls the tools as a plugin would. It checks that the
  view matrix maps the eye to the origin, that a stroke and a new piece show
  up as events, and that the geometry file matches the mesh.
- An app-level test runs a real Python plugin against the live loopback
  listener. The plugin reads the state, follows changes across a mask fill
  and a new piece, and reads geometry through the SDK.
