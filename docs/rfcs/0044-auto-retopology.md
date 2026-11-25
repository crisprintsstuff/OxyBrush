# RFC 0044 — Automatic quad retopology (field-aligned)

- **Status:** Phase 1 implemented (further phases open)
- **Issue:** [#529](https://github.com/crisprintsstuff/OxyBrush/issues/529) (Autoremesher request)
- **Name in UI:** **Auto-retopology** (generic; not a third-party product name)

## Problem

Voxel remesh and quadric decimation rebuild density, but they do not
produce clean, feature-aligned quads for animation, UVs or subdivision.
Manual retopology ([RFC 0012](0012-retopology.md)) is the right tool for
eyes and joints; artists also want a one-shot automatic pass that turns a
dense sculpt into a lighter, mostly-quad mesh that follows the form.

The ROADMAP already names the Instant Meshes / QuadriFlow research line for
this. Discord requested Autoremesher ([#529](https://github.com/crisprintsstuff/OxyBrush/issues/529)),
an MIT-licensed desktop tool built on C++ stacks (Geogram, libigl, …). OxyBrush
cannot ship that stack: **pure Rust**, no C/C++ toolchain, and **no GPL**
dependencies or ports (Instant Meshes’ reference code and CoMISo are
copyleft; we use published papers only — see
[CLEAN_ROOM.md](../CLEAN_ROOM.md)).

## Decision

Add **Mesh > Auto-retopology…** (command palette too): a background job that
replaces the active piece with a field-aligned remesh, one undo step.

### Algorithm (Phase 1)

Implement the **Instant Field-Aligned Meshes** pipeline
(Jakob, Tarini, Panozzo, Sorkine-Hornung, SIGGRAPH Asia 2015) in
`oxy-topo`, from the paper’s local smoothing description:

1. **Graph** from the input triangle mesh (vertex positions, normals, 1-ring).
2. **Orientation field** (4-RoSy for quads): extrinsic Gauss–Seidel with
   nested brute-force integer rotations (paper §3.2).
3. **Position field** (4-PoSy): extrinsic GS with integer translations and
   lattice rounding to target edge length ρ (paper §3.3).
4. **Multiresolution hierarchy** by area-weighted vertex clustering
   (paper §3.5) so GS escapes poor local minima.
5. **Extraction**: collapse zero-jump clusters, keep unit-jump edges as the
   output graph, walk faces (paper §3.8). Quad-dominant; optional one
   Catmull–Clark-style split later for pure quads (Phase 2).

Controls (Phase 1):

| Control | Meaning |
|---|---|
| Target faces | Approximate face count (sets ρ from surface area) |
| Sharp angle | Dihedral threshold for crease normals (degrees) |
| Extrinsic | Prefer embedding-space smoothness (default on) |

### Placement

- Algorithm in **`oxy-topo`** (`auto_retopo` module): GPU-free, pure Rust,
  same crate as voxel remesh / decimate.
- App: `Editor::auto_retopo_active` via `start_job`, like remesh.
- Do **not** link Autoremesher, Geogram, libigl, Instant Meshes binaries, or
  any GPL solver.

### Naming

UI, docs and code say **auto-retopology** / `auto_retopo`. Autoremesher may
be named only when identifying the issue request or comparing features
(same rule as ZBrush format mentions).

## Non-goals (Phase 1)

- Interactive orientation / position brushes and singularity editing (§3.7)
- Point-cloud input
- Matching Autoremesher’s CLI flag set 1:1
- Pure-quad guarantee without a subdivision post-pass
- Attribute transfer beyond what remesh already does (mask / groups /
  vertex color: follow remesh policy; Phase 1 may drop paint like early remesh)

## Phases

| Phase | Ships |
|---|---|
| **1** | Background auto-retopo job; options above; quad-dominant output; unit tests on closed primitives; CLEAN_ROOM + ROADMAP |
| **2** | Pure-quad post-pass; attribute projection onto the new mesh; better sharp-feature handling; timing example |
| **3** | Optional orientation stroke constraints; singularity attractor; adaptivity / anisotropy knobs |

## Verification

- `cargo test -p oxy-topo auto_retopo`
- Background job + undo in `oxy-app` editor tests
- AGENTS checks before push
- Visual check on a dense sculpt (headless screenshot when practical)

## Sources (clean-room)

- Jakob et al., *Instant Field-Aligned Meshes*, SIGGRAPH Asia 2015
  ([project page / PDF](https://igl.ethz.ch/projects/instant-meshes/))
- Issue [#529](https://github.com/crisprintsstuff/OxyBrush/issues/529) and
  Autoremesher’s public README (MIT tool identity and CLI knobs only —
  no C++ algorithm source consulted for this port)
- QuadriFlow remains a Phase 2+ research option; Phase 1 follows Instant Meshes
