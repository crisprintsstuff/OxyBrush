# RFC 0010: Blender bridge

- **Status:** proposed for the maintainer; design only, nothing implemented yet
- **Requested by:** testers, who want to send a model from Blender to OxyBrush,
  sculpt it, and get it back without exporting and importing by hand.

## Problem

Today a Blender user exports an OBJ, imports it into OxyBrush, sculpts,
exports again, re-imports in Blender, and then fixes up the object: its
name, location, modifiers, materials, UVs and shape keys are not on the
re-imported copy. Every round trip costs a minute and loses work.

## Goals

- **One click each way:** "Send to OxyBrush" in Blender; **Send back** in
  OxyBrush (the button that already exists for OxyCharacter documents).
- **Keep the Blender object:** when the vertex count is unchanged (posing,
  sculpting detail without dynamic detail or remesh), Blender updates the
  vertex positions of the same mesh, so UVs, materials, modifiers, vertex
  groups and shape keys survive. When the topology changed, the mesh data
  is replaced on the same object, with a message saying what was lost.
- **Several objects at once,** each a piece named after its object.
- **No new network access** in OxyBrush and no change to its rules.

## Decision

### Transport: files and the existing document link

OxyBrush already round-trips with OxyCharacter: a document opened next to a
`<file>.oxylink.json` sidecar shows **Linked to OxyCharacter**, and **Send
back** saves it where the other app is watching. The bridge generalizes
that link instead of adding a new channel:

1. **Send to OxyBrush (Blender):** the add-on exports the selected mesh
   objects, with modifiers off and in object space, to a folder of its own
   (`<temp>/oxybrush-bridge/<session>/`): one OBJ per object, written to a
   temporary name and renamed. It writes a manifest
   `bridge.oxylink.json` naming the app (`"app": "Blender"`), each object's
   name, OBJ path and world transform, and where OxyBrush should write
   results. It then starts OxyBrush with that manifest
   (`oxybrush --link <manifest>`), or, if OxyBrush is already running with
   that link open, rewrites the OBJs and OxyBrush offers to reload them.
2. **OxyBrush** opens the manifest like a document: one piece per object,
   named after it, placed by its world transform so the pieces sit as they
   did in Blender. The vertex order of each OBJ is kept (the `VMAP`
   section added for OxyCharacter does this already). The badge reads
   **Linked to Blender**.
3. **Send back (OxyBrush):** each piece whose name matches an object is
   exported, back in its object's space, to the result path the manifest
   gives, atomically (temporary file and rename), in the original vertex
   order when the topology is unchanged. Pieces added in OxyBrush are
   written too and listed in a small `returned.json`, so Blender can add
   them as new objects.
4. **Blender** watches the result folder with a timer (a second or so) and
   applies what arrived: same vertex count → update positions in place;
   otherwise replace the mesh data and report it. Each update is one
   Blender undo step.

OxyBrush never reads anything from Blender but the manifest and the OBJs it
names, never writes outside the result paths the manifest gives (which must
be inside the manifest's folder), and never deletes Blender's files.

### Why not MCP

The local MCP server ([RFC 0003](0003-mcp.md)) could carry the same data,
but it is off by default, needs a per-session token pasted into the add-on,
and would put whole meshes through JSON. Files need nothing switched on,
work when OxyBrush isn't running yet, and reuse a mechanism that is already
tested. MCP stays the route for automation and agents.

### Licence and where the add-on lives

Blender add-ons import Blender's Python API, and Blender's position is that
add-ons are covered by its GPL. OxyBrush is MIT/Apache-2.0 and never takes
GPL code. So the add-on lives in a **separate repository**
(`oxybrush-blender`), licensed **GPL-2.0-or-later**, and is written from
scratch from Blender's public API documentation. No existing bridge add-on's
source is read (see [CLEAN_ROOM.md](../CLEAN_ROOM.md)). This repository only
gains the generalized link, the `--link` flag and documentation of the
manifest format, all MIT/Apache-2.0. The manifest format is the contract
between the two, so other apps can use it too.

## Work in OxyBrush

- Generalize `Link` from OxyCharacter to any app named in the sidecar
  (badge text, Send back target), keeping OxyCharacter's files working
  unchanged.
- `--link <manifest>`: open the OBJs as pieces, with transforms and names.
- Send back for linked OBJ pieces: per-piece export in object space, in
  the original vertex order, atomic writes, `returned.json`.
- Reload when the linked OBJs change while open (asks first if there are
  unsaved edits).
- Tests: a manifest round trip in `oxy-io`/`oxy-app` (names, transforms,
  vertex order kept, new pieces listed, writes refused outside the folder).

## Open questions

- OBJ or glTF for the exchange? OBJ is simplest and keeps vertex order;
  glTF would carry vertex colors and UVs. Proposed: OBJ first, glTF later.
- Should Send back also offer "Send back and close"?
- Who maintains the add-on repository, and how is it released (Blender
  extensions platform, or a zip on GitHub releases)?
