# RFC 0022: Surface instances (plan)

Status: Partial — phase 1 implemented in 4.0.0-beta.12 (rigid copies on faces, points and face groups, Edit source, Make real, `INST` section). Phase 2 adds masked placement, even scatter on visible faces, scatter on mask/group (`INS3`), and Flow orientation. Remaining: edges, tiling, hand exclusions, fitted copies and glTF instancing export.

## Summary

Copy one piece (the **source**) onto another piece (the **host**) as live
**instances**: one per face, per point, per edge, only on a face group or
the masked part, or scattered evenly at a chosen density. Every copy
follows the host as it is sculpted, and editing the source updates every
copy at once. Copies can sit on the surface as rigid objects (rivets,
spikes, scales, studs, hair cards) or be **fitted** into each face,
bending with it (chainmail, woven panels, tiles, armor plates). When the
look is right, **Make real** turns them into ordinary mesh.

The name is generic on purpose: other products' names for this feature
are trademarks (AGENTS.md, "Trademarks").

## Why

Testers keep asking for it: repeated detail is slow to sculpt, impossible
to keep consistent by hand, and heavy as real geometry. One rivet sculpted
once and placed on 2,000 faces is a few kilobytes of placement data; the
same as real mesh is millions of triangles.

## What others do (public documentation only)

- A sculpting app places one copy per polygon of a placement mesh, with
  alignment to the normal, to the polygon's point order, or to its short,
  long or a random edge; size relative to the polygon, "fit" into it, or a
  constant height; tiling several copies per polygon; random placement;
  and random offset. Its documentation warns that point order differs
  between neighbouring polygons, so copies aligned by point order don't
  all face the same way.
- Node-based tools place copies on points and build each rotation from a
  direction: the normal as one axis and an **up** vector as another. Their
  documentation warns that without an explicit up the guess is often
  wrong. They offer a step to turn instances into real geometry.
- glTF has a standard extension, `EXT_mesh_gpu_instancing`, storing one
  translation, rotation and scale per copy; three.js loads it as instanced
  meshes.
- Even random scattering on a surface is Poisson-disk sampling; Cline et
  al., *Dart Throwing on Surfaces* (Computer Graphics Forum, 2009) do it
  directly on triangle meshes. Fitting a shape into a surface patch, with
  height along interpolated normals, is the idea of *shell maps*
  (Porumbescu et al., SIGGRAPH 2005).
- GPU instancing saves draw calls, not triangles: a million copies of a
  5,000-triangle source is still 5 billion triangles to draw.

## Design

### Data

An **instancer** belongs to the host piece (a host can have several):

- `source`: a piece id. The source piece is hidden and isn't sculpted
  directly from the Pieces list: **Edit source** (a mode on the host's
  instancer) shows it alone, in its own place, to edit it.
- `place`: **faces** (each polygon, quads kept from the piece's
  polygons when it has them), **points**, **edges** (copies stretched or
  centred along each edge), **group** (faces of one face group),
  **mask** (where the mask is above a threshold), or **scatter**
  (Poisson-disk at a spacing, on all, a group, or the masked part).
- `orient`: the copy's up axis follows the surface **normal**; its
  forward axis is a choice in each instancer: a **direction** in the
  world (the default: copies never flip between neighbours, but can look
  turned on a strongly curved host), the face's **longest edge**
  (follows the surface, can flip between neighbours), its **first
  edge**, or a **random** spin. Mirroring follows the host's symmetry, so
  left and right copies match.
- `size`: absolute (scene units), relative to the face, or **fit**
  (see below); plus **height** separately, so studs don't grow taller on
  bigger faces.
- `offset` along the normal, `tile` (copies per face, in rows and
  columns), and `vary`: random scale, spin and offset, with a seed so
  the result is the same every time.
- `exclude`: a list of placements turned off by hand.

Placements are stored as **anchors** (face or point index, and
barycentric position), not as transforms, so they move with the host when
it is sculpted. When the host's topology changes (remesh, subdivide,
dynamic detail), anchors are rebuilt from the rule; hand exclusions are
kept by matching positions where possible and dropped otherwise, with a
note.

### Two kinds of copy

- **Rigid** copies have a transform each (translation, rotation, scale),
  rebuilt from the anchors and the host's current surface. They're drawn
  with GPU instancing: the source's vertex buffer once, and a per-instance
  buffer of transforms.
- **Fitted** copies take the shape of their face: the source's bounding
  box is mapped onto the face (bilinear across a quad, barycentric across
  a triangle), and its height runs along the face's interpolated point
  normals, so a strip of chainmail bends with the surface (the shell-map
  idea). This is done in the vertex shader from per-instance corner
  positions and normals, so copies stay instanced and live.

### Editing

- **Edit source**: shows the source alone, in place, and sculpting,
  Poly Edit, painting and the gizmo work on it as on any piece; every copy
  updates as you go.
- **Edit placement**: hides the copies and shows the host, to sculpt or
  re-group it; the copies return on the new surface.
- Click a copy with the Instances tool to turn it off (or back on); Shift
  to toggle a stroke's worth.
- Each change is one undo step (the instancer's settings, the exclusions,
  or the source piece's own undo).

### Make real and export

- **Make real** writes the copies into a new piece (or into the host),
  one face group per copy, through the normal add-piece and topology undo.
  A count and triangle estimate are shown first; above the triangle limit
  it asks. The instancer and its source are **kept** (hidden), so after
  changing the source or the host you can make it real again; Remove
  instancer deletes them when you're done.
- **glTF/GLB export**: rigid copies as `EXT_mesh_gpu_instancing` on a
  node (tiny files, three.js and engines keep them as instances), or
  realized, by an export option, since not every importer reads the
  extension. Fitted copies are always realized.
- **OBJ, STL and PLY**: realized.
- **Blender bridge**: realized, so the round trip stays plain meshes.

### Saving

A new skippable `INST` section per host: the settings, the source piece's
id, the seed and the exclusions. Older versions skip it and open the host
and the (hidden) source as ordinary pieces, nothing lost.

### Performance

- Rigid copies: one instanced draw per instancer. The per-frame cost is
  the triangles drawn: shown in the instancer as "N copies, M triangles",
  and above a budget (set in Preferences) copies far from the camera are
  drawn as a simplified source (a decimated level of detail, built once
  when the source changes).
- Re-anchoring after a host stroke runs only for the chunks the stroke
  touched, through the same `update_after_move` path the brushes use.
- Scatter sampling and Make real run as background jobs (invariant 5).
- A `instance_timing` example reports placement, re-anchoring and
  draw cost for 1k, 10k and 100k copies.

## Phases

1. **Rigid copies on faces, points and groups**, with normal + world
   direction orientation, size, offset and random variation; live source
   editing; Make real; saving. (Most of the value.)
2. **Scatter** (Poisson-disk on the surface), mask placement, edges,
   tiling, hand exclusions.
3. **Fitted copies** (shell mapping in the vertex shader).
4. **glTF instancing export**, levels of detail, agent and plugin access
   (place, change, make real).

### Phase 2 implementation

- **Masked surface** uses one anchor per visible polygon. Copies are
  drawn where the polygon's average corner mask meets the live threshold.
- **Even scatter** samples visible triangles by area in a background job,
  accepting seeded points at the requested minimum scene-space distance.
  The sampler caps output at 20,000 copies and keeps accepted anchors as
  the surface deforms. Changing the seed, spacing, or host topology
  generates new anchors.
- `INS2` carries the full list for hosts that use either new mode. Older
  readers still load supported modes from `INST`.

## Not in this plan

- Copies of copies (an instancer whose source has its own instancer).
- Physics or collision between copies.
- Sculpting one copy differently from the others (make it real first).

## Decisions

- **Edit source** is a mode on the host's instancer; the source stays a
  hidden piece.
- **Orientation** is chosen per instancer (world direction, longest
  edge, first edge, random); world direction is the default.
- **Make real** keeps the instancer and its source.
