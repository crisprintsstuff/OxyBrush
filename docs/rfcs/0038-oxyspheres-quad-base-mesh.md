# RFC 0038 — OxySpheres quad base mesh

**Status:** Draft, follow-up to [RFC 0034](0034-oxyspheres.md)

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

Higher-valence junctions, branch-specific controls, and artist-adjustable
ring density are follow-up decisions. Intersecting limbs, coincident joints,
and links too short to fit a junction are explicit rejection cases for the
first prototype.

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
subdivision checks. Surface validation also rejects intersections between
triangles that do not share a vertex. Inspect the sample junction profiles
before expanding the supported armature types.

## Alternatives

Retopologizing the voxel result could handle overlapping limbs more
naturally, but adds a separate topology pass and makes it harder to
guarantee loops that follow the original armature. Keep it as a candidate
for more complex armatures after evaluating direct ring construction.
