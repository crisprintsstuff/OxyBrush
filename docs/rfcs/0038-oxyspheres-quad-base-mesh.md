# RFC 0038 — OxySpheres quad base mesh

**Status:** Accepted. Quad skin supports joints with up to six links, Subdivision, Resolution, and Blend. Follow-up to [RFC 0034](0034-oxyspheres.md).

## Summary

Generate a low-density, mostly quad base mesh from an OxySpheres
armature. The same sphere centers, links, and radii drive both the proposed
**Quad base mesh** and the existing voxel skin.

## Motivation

A voxel skin gives artists a continuous surface to sculpt immediately.
Artists who want to subdivide a sparse base mesh benefit from edge loops
that follow each limb. Those loops give them a coarse level for changing
proportions and a predictable cage for subdivision.

Blender's public Skin manual documents a similar input and output, including
triangles near intersections and the trade-off between smoothing branch
junctions and shrinking their profile. Use that documented behavior as a
reference for evaluating the result. For algorithm research, start with
Ji, Liu, and Wang, *B-Mesh: A Fast Modeling System for Base Meshes of 3D
Articulated Shapes* (Pacific Graphics 2010), as cited in RFC 0034.
[Blender Skin manual](https://docs.blender.org/manual/en/latest/modeling/modifiers/generate/skin.html)

## Design

### Headless prototype

1. Start with one tapered, bent chain. Place cross-section rings along the
   links, interpolate radii, and carry a consistently oriented frame across
   bends to limit twist. Bridge consecutive rings with quads and close the
   ends. Use a fixed, even ring size for the prototype.
2. Add one three-way junction. Reserve space for the junction along each
   incident link and join the three ring boundaries with a local polygon
   patch. Keep triangles local to junctions and caps where they improve
   the shape. Compare at least two branching angles and a small branch
   attached to a thick trunk before choosing a junction construction.
3. Return explicit polygon connectivity through the existing mesh polygon
   representation so Poly Edit, Catmull–Clark subdivision, and OBJ export
   retain the intended face boundaries. Keep the generator in the
   GPU-free core and use synthetic armatures for regression fixtures.

### Editor integration after the prototype passes

Offer **Quad base mesh** as a choice in Make mesh while keeping the current
voxel skin as the default. Generate and validate the mesh on a worker.
Use the existing generated-piece and undo behavior. If the armature cannot
produce a valid quad skin, explain the problem and leave the armature and
existing mesh intact. The user can choose the voxel skin for that armature.
Keep the output choice session-local initially. The armature and generated
mesh use the existing project representations.

Higher-valence junctions and ring density are covered in the follow-ups below.
Branch-specific controls remain future work. Intersecting limbs, coincident joints,
and links too short to fit a junction are explicit rejection cases for the
first prototype.

### Follow-up controls

Quad output exposes **Subdivision**, from 0 for the base cage to 2 for
a smoother initial surface. Catmull–Clark runs on the worker, with a
16,384-face budget for subdivided output. The result is validated before
replacing a mesh. The optional `ARQD` project section remembers both quad
output and subdivision level. Older readers can skip it and still load
the armature and mesh.
This scales detail relative to the base cage, so a longer chain still has
more total faces than a short one.

**Resolution** (same slider as voxel skin) adds rings along long links
above 160, including between junction collars.
**Blend** (0..=1) softens three-way junctions by moving collar rings out
from the joint; `0.5` matches the previous fixed collar. Endpoints and
limb radii stay put. Acute, wide and mirrored forks are validated across
the range.

### Hand and foot junctions

Extend the direct ring construction to four through six links per sphere.
The incoming forearm or ankle takes one port and up to five digits can attach
to that same joint. Keep the socket radii close to the incoming limb and
digit radii so a broad joint does not force every digit to start at palm
width. Narrow sockets when short links need extra clearance. Move the collars
outward as needed until each is a separate face of
the junction hull. Reserve part of each link for the limb strip, including
when it leads directly to another junction. Keep the existing three-way
collar placement for older cages.

Measure fit against the planes of the proposed collars. A small set of
points inside those planes supports the bulk of the palm or heel as the cage
subdivides. Remove the collar faces from the hull before the existing quad
sweeps connect the digits. Validate the cage and the subdivided result
through the existing mesh checks. Closely packed or overlapping digits can
still require more room.

## Acceptance criteria

- For a straight taper, a bent chain, a mirrored fork, and a wide and an
  acute Y-junction, verify closed oriented manifold connectivity, no
  zero-area faces or self-intersections, and deterministic face connectivity.
  Mirror-paired branches should have matching loops and mirrored positions.
- Check that limb strips remain quads around the full circumference.
  Inspect the cage and two Catmull–Clark levels for twisted strips,
  visible junction pinching, and collapsed tips. Record face counts and
  before/after views so the quality decision can be reviewed.
- Verify that unsupported fixtures fail without producing an editable
  replacement mesh. If the Y-junction does not meet the topology and shape
  checks, keep the work headless and revisit the junction construction.
- Before exposing the mode in the editor, check one undo/redo cycle,
  project save/reopen, Poly Edit picks, and quad OBJ export.

## Prototype results

Run `cargo run -p oxy-topo --example quad_skin_fixtures -- OUTPUT_DIRECTORY`
to export the base cages and level-two meshes as OBJ. The bent taper has
24 quads. Each fork has 40 quads and 18 triangles, with triangles confined
to the junction. After two levels, the taper has 384 quads and each fork
has 856. The test fixtures pass orientation, mirror connectivity, and
subdivision checks. An asymmetric, nonplanar fork with a bent side branch
also passes at two levels. Surface validation rejects intersections between
triangles that do not share a vertex. Inspect the sample junction profiles
before expanding the supported armature types.

The connected-junction follow-up sweeps one quad strip between the facing
collars, including through intermediate bends. Separate hull patches keep
triangles local to each junction. An armature with two forks subdivides
twice while staying closed. If the collars consume the shared link, the
worker rejects the build and keeps the armature open for editing.

The five-finger hand fixture has 112 quads and 62 triangles in its cage.
Two subdivision levels produce 2,536 quads. Tests exercise wider and
narrower digit fans, mirrored hands, and a five-toe ankle. They also check
that crowded links still fail before replacing a mesh.

## Alternatives

Retopologizing the voxel result could handle overlapping limbs more
naturally, but adds a separate topology pass and makes it harder to
guarantee loops that follow the original armature. Keep it as a candidate
for more complex armatures after evaluating direct ring construction.

Mirrored limb quads use matching triangulation diagonals for viewport shading.
A successful rebuild reveals the updated output, and undo restores its prior visibility.
