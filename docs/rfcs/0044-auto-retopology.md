# RFC 0044 — OxyRemesh (field-aligned automatic retopology)

- **Status:** Phase 3 implemented; Phase 4 (Autoremesher-standard extract) in progress
- **Issue:** [#529](https://github.com/crisprintsstuff/OxyBrush/issues/529) (Autoremesher request)
- **Name in UI:** **OxyRemesh** (OxyBrush product name for field-aligned remesh)

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

Add **Mesh > OxyRemesh** (command palette too): a background job that
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
   output graph, walk faces (paper §3.8).
6. **Phase 2 post-pass**: Catmull–Clark *topology* (face + edge midpoints,
   no smoothing) so every polygon becomes quads; snap vertices onto the
   source; `transfer_attributes` for color / mask / groups; crease edges
   get higher weight and orientation pull during GS.

Controls:

| Control | Meaning |
|---|---|
| Target faces | Approximate face count (sets ρ from surface area; accounts for pure-quad split) |
| Sharp angle | Dihedral threshold for crease normals (degrees) |
| Extrinsic | Prefer embedding-space smoothness (default on) |
| Pure quad | Split every extracted polygon into quads (default on) |
| Transfer attributes | Carry paint, mask and face groups (default on) |
| Snap to surface | Project result onto the source mesh (default on) |
| Adaptivity | 0..1 denser quads in high-curvature regions (Phase 3) |
| Anisotropy | 0..1 stretch cells along the orientation field (Phase 3) |
| Guide from last stroke | Soft orientation constraints from the last freehand stroke |
| Attract poles to mask | Soft singularity attractor at the mask centroid |

### Placement

- Algorithm in **`oxy-topo`** (`auto_retopo` module): GPU-free, pure Rust,
  same crate as voxel remesh / decimate.
- App: `Editor::oxyremesh_active` via `start_job`, like remesh.
- Do **not** link Autoremesher, Geogram, libigl, Instant Meshes binaries, or
  any GPL solver.

### Naming

UI and docs say **OxyRemesh**. The algorithm module is `oxy_topo::auto_retopo`
(Instant Meshes paper). Autoremesher may be named only when identifying the
issue request or comparing features (same rule as ZBrush format mentions).

## Non-goals (through Phase 3)

- Full interactive field-paint brush tool (Phase 3 uses last-stroke guide + mask attractor)
- Point-cloud input
- Matching Autoremesher’s CLI flag set 1:1
- Full Catmull–Clark *smoothing* (Phase 2 only uses CC connectivity)
- QuadriFlow solver (still a research option)

## Phases

| Phase | Ships |
|---|---|
| **1** | Background OxyRemesh job; field pipeline; quad-dominant extraction; unit tests; CLEAN_ROOM + ROADMAP |
| **2** | Pure-quad post-pass; attribute projection + surface snap; crease-weighted GS; `oxyremesh_timing` example |
| **3** | Orientation stroke constraints (guide from last stroke); singularity attractor (mask centroid); adaptivity / anisotropy knobs under Topology > OxyRemesh |
| **3b** | Extraction hardening (#551): face-walk edge commit, paper §3.8 low-support filter, keep-largest floater strip |
| **4** | Autoremesher-standard path (MIT): isotropic resample (default on), dual-extract hole fill / non-manifold / short-edge cleanup + seal fallback, isoline quad extract (gated until coverage is good), global UV/frame parameterization (pure-Rust MIQP, no Eigen — still open). Refuse open surfaces on closed inputs when seal cannot close. |

The isoline extractor accepts per-corner UVs and splits each isoline at
interior integer crossings before welding the graph. For independent charts,
both endpoints of every shared edge must agree under a single quarter turn
and integer translation. That gives future solvers an extraction contract
to test on open and closed fixtures. The first field fit combs equivalent
quarter turns along the vertex graph. It rejects any edge that still needs a
different quarter turn after combing and relaxes a least-squares fit of UV
edge differences. Triangle charts use integer offsets of those shared
coordinates. Open inputs can use the fitted charts when they are unfolded and pass the
face-count and boundary checks. Closed inputs still use the existing lattice
and dual-extraction paths. Cutting at field
singularities, solving for independent chart transitions, and measuring
coverage on curved sculpts remain open.

The cut-topology builder splits triangle corner fans at designated interior
edges and pairs the resulting UV endpoints by source vertex. This gives the
next solver separate variables on either side of a cut without changing the
sculpt mesh. The preparation stage seeds these cuts from residual quarter turns on
interior edges after combing. A spanning forest of the remaining face adjacencies keeps each chart a
disk. This can add more cuts than a shortest-cut method. A seam fit now enforces supplied integer translations and quarter turns
on the cut vertices. It projects an initial UV estimate onto the shared
constraints and rejects inconsistent cycles. Computing the transitions
and integrating this fit with the orientation-field solve remain open.

## Verification

- `cargo test -p oxy-topo auto_retopo`
- Background job + undo in `oxy-app` editor tests
- AGENTS checks before push
- Visual check on a dense sculpt (headless screenshot when practical)

## Sources (clean-room)

- Jakob et al., *Instant Field-Aligned Meshes*, SIGGRAPH Asia 2015
  ([project page / PDF](https://igl.ethz.ch/projects/instant-meshes/))
- Issue [#529](https://github.com/crisprintsstuff/OxyBrush/issues/529) and
  Autoremesher ([huxingyi/autoremesher](https://github.com/huxingyi/autoremesher),
  MIT since 1.0.0). Phase 1–3 used the README only; Phase 4 studies the MIT
  algorithm sources (isoline extract, hole fill, cleanup). Instant Meshes
  reference C++ remains GPL and is not opened. Eigen is not linked (pure Rust
  solvers for any MIQP port).
- QuadriFlow remains a research option alongside Phase 4
