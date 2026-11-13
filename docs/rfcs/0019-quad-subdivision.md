# RFC 0019: Quad subdivision levels (Catmull–Clark)

Status: accepted and implemented (subdivision levels, Subdivide, retopology
Subdivide and project). UVs come next, in [RFC 0020](0020-uvs.md).

## Summary

Subdivision levels (Ctrl+D), **Mesh > Subdivide** and the retopology tool's
**Subdivide and project** make **quads** by default, with Catmull–Clark
subdivision. Every polygon of the base becomes quads, so every level above
the base is all quads and the base's edge loops carry on up. **Triangles**
(Loop subdivision, what OxyBrush did before) stays available as an option.

## Why

Loop subdivision splits each triangle into four. That's fine for sculpting,
but a quad base mesh, such as the human base meshes, a retopology mesh or a
Poly Edit model, loses its quads at the first level: its edge loops are
gone and an OBJ exported from a higher level is all triangles. Work that
leaves OxyBrush (UV unwrapping, UDIM texture baking, animation rigs, other
sculpting and modeling apps) expects quads that follow the base's loops.
Most sculpting apps that keep subdivision levels use Catmull–Clark for this
reason; OxyBrush already had it for Poly Edit's smooth preview.

## What the user sees

- **Topology > Subdivision levels** shows **Makes: Quads | Triangles** while
  the piece has no levels. Quads is the default, and the choice is remembered
  (`subdivision = quads | triangles` in the settings file). Once a piece has
  levels it keeps their kind, shown in the same place; deleting the levels or
  remeshing lets it change.
- The same choice applies to **Mesh > Subdivide (single mesh)** (it shows
  under that button in the Mesh menu too) and to the retopology tool's
  **Subdivide and project**.
- On a quad base, each level has four times the triangles, as before. A
  triangle in the base becomes three quads (six triangles); from level 2 on
  it's ×4 again. The triangle limit and the estimate account for that.
- The status bar's quad count is exact on quad levels, and OBJ export writes
  their quads.
- Poly Edit creases carry into the levels: a crease at 100% stays sharp at
  every level, lower weights round off after a level or two.

## How it works

`oxy_mesh::multires::Multires` has a `Scheme`: `CatmullClark` or `Loop`.

**The base polygons.** A Catmull–Clark stack reads the base as polygons:
the piece's own (`Piece::current_cage`, kept by Poly Edit, the retopology
tool, the base meshes and quad primitives), or otherwise triangle pairs the
mesh already counts as quads (`Mesh::quad_diagonals`), with leftover
triangles as triangles. Triangles are joined across the edges that aren't
polygon edges; a group whose outline isn't one simple loop stays as
triangles. Each polygon keeps the list of base triangles it is made of, so
the base mesh itself is unchanged and face groups and visibility map both
ways.

**Levels.** One Catmull–Clark step makes, after the old points (moved), one
point per edge and one per face, and turns a face with `m` corners into `m`
quads, in face order. Each quad `[a, b, c, d]` is the level's triangles
`2q` (`a b c`) and `2q + 1` (`a c d`), so brushes, the renderer, undo and
Living strokes see an ordinary triangle mesh, while the quads are known
exactly (`bind` tells the mesh its quad diagonals). The rules are those of
Catmull & Clark (1978) with the semi-sharp creases of DeRose, Kass & Truong
(SIGGRAPH 1998), as in `catmull_clark.rs`; a test checks that both give the
same points. Edges are numbered in first-seen order with a parallel sort,
like the Loop levels, because saved positions depend on it.

**Detail** works as before: `detail[k] = positions[k] − smooth(positions[k−1])`
in object space, spread down on sync and rebuilt upward.

**Attributes.** Mask and color of new points are the average of the points
they come from. Each quad takes its coarse face's group, and is hidden when
the whole face was; going down, a face takes its first quad's group and is
hidden when all its quads are.

**Creases** are stored for the base (`[a, b]`, weight 0..1) and carried up as
sharpness `4 × weight`, one less per level. **Delete lower levels** gives the
new base the creases its edges have at that level.

## Saving

A new skippable section, `MRCC`, replaces `MRES` for quad levels. It starts
with exactly `MRES`'s layout (base triangles and every level's positions),
then the base polygons (corners and triangles per face) and the creases.
Older versions don't know `MRCC`: they skip it and open the piece's active
level as a plain mesh, as they do with levels they can't match. Projects
with triangle levels keep saving `MRES` and open as before.

## Performance

Measured with `multires_timing` (4 threads; an 82k-triangle icosphere with
Loop levels against a quad sphere of the same size, three levels up to about
5.2M triangles):

| | Loop | Catmull–Clark |
|---|---|---|
| top level, stack | 411 ms | 423 ms |
| top level, mesh build | 457 ms | 684 ms |
| status bar quad count | 839 ms (guessed from the shape, on the UI thread) | 0 ms (known) |
| finding base polygons | – | 66 ms |
| undo copy of the stack | 109 ms | 133 ms |

The mesh build includes telling the mesh its quads, which Loop meshes pay
later on the UI thread when the quad count is first shown. Quad levels keep
16 bytes per quad more than Loop levels. Sculpting itself is unchanged: the
mesh is the same kind of triangle mesh.

## Not included

- UVs: see [RFC 0020](0020-uvs.md).
- Converting a piece's existing levels from one kind to the other.
- Limit-surface evaluation (pushing points to where infinite subdivision
  would put them); levels show the subdivided points, as before.
