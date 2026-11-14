# RFC 0021: Gaussian splats, as reference, mesh and sculpt, with a Blender round trip (plan)

Status: proposed

## Summary

OxyBrush opens Gaussian splat captures (the scans that phones and
photogrammetry tools now produce as millions of soft, colored blobs) and
lets you:

1. **see one as a reference** next to your sculpt, lined up with the
   transform gizmo, faded and cropped;
2. **convert it to a mesh** that sculpts like any other piece, with its
   colors as vertex color;
3. **sculpt the splats themselves**, by moving them with a mesh you sculpt
   (and, for clean-up, a few brushes that act on the splats directly);
4. **send splats and meshes to Blender and back** through the Blender
   bridge (RFC 0010).

Each step is useful alone and lands as its own change, in that order.

## Why

Testers scan real objects (heads, props, sculptures) and want to sculpt
over them or from them. Today they have to convert a scan to a mesh in
another tool first, and lose its look. Splat captures are now a common
output of capture apps, and Blender users exchange them as PLY files.

## What a splat cloud is

Each splat is a 3D Gaussian: a centre, a size along three axes, a
rotation, an opacity and a color that can change with the viewing
direction (spherical harmonics). Drawn sorted back to front and blended,
millions of them reproduce a captured scene. Reference: Kerbl, Kopanas,
Leimkühler, Drettakis, *3D Gaussian Splatting for Real-Time Radiance
Field Rendering*, SIGGRAPH 2023; and Zwicker et al., *EWA Splatting*,
IEEE TVCG 2002, for projecting a 3D Gaussian to the screen.

## Plan

### Data and files

- A scene gains **splat pieces** beside mesh pieces: shown in the Pieces
  panel with their own icon, with visibility, name and transform like any
  piece. A splat piece holds per splat: centre, log-scale (3), rotation
  (quaternion), opacity, base color (spherical harmonics degree 0) and,
  optionally, the higher-degree view-dependent color (degrees 1 to 3),
  kept as half-floats to halve memory. About 60 bytes per splat with
  degree 3 color, so one million splats is about 60 MB.
- **Import:** the common PLY layout (`x y z`, `f_dc_*`, `f_rest_*`,
  `opacity`, `scale_*`, `rot_*`), the `.splat` layout, and `.spz`
  (written from its published format description, in pure Rust). A PLY
  with those properties opens as splats; any other PLY still opens as a
  mesh. The glTF splat extension follows once Khronos finalizes it.
- **Export:** the same PLY layout, so the result opens in other splat
  tools and in Blender add-ons.
- **`.oxb`:** a new skippable `SPLT` section per splat piece (compressed,
  like other sections). Older versions skip it and open the project
  without the splats; Save in those versions would drop them, so the
  release notes say so.

### Phase A: see it as a reference

- A **splat pass** in `oxy-render`: each splat is drawn as a screen-space
  ellipse from its projected 3D Gaussian (EWA), sorted back to front and
  blended over the meshes, which it is depth-tested against so a sculpt
  can stand inside a scan.
- **Sorting** happens only when the view changes, off the UI thread
  (invariant 5), with a radix sort on depth; the frame draws the last
  sorted order meanwhile. A GPU sort is a later step if measurements ask
  for it. A new `splat_timing` example reports draw and sort cost for 1, 3
  and 6 million splats; the per-dab path is untouched.
- **Reference controls:** Opacity, Crop box (hides splats outside it,
  non-destructively), Lock (not picked by brushes or clicks), and the
  transform gizmo to line the scan up with the model.
- **Floaters:** Select by opacity or by distance from the main cluster,
  then Delete, to clean up stray blobs common in captures. Undoable.

### Phase B: convert to a mesh

- **Splats > Make mesh…** evaluates the splats' opacity as a density field
  on a voxel grid (each Gaussian adds its density to the cells it covers),
  then extracts the surface at a chosen level with the existing voxel
  remesh polygonizer (`oxy-topo`), and gives each point the color of the
  splats around it as vertex color.
- **Options:** resolution (with the triangle estimate the voxel remesh
  already shows), density level, keep only the largest part, and the crop
  box. Runs as a background job with progress and Cancel.
- Expect a soft, closed surface, good as a base to sculpt over, not a
  clean scan of fine detail: splats describe appearance more than shape.
  Methods that recover sharper surfaces (for example SuGaR, Guédon and
  Lepetit, CVPR 2024; 2D Gaussian Splatting, Huang et al., SIGGRAPH 2024)
  change how splats are captured or optimized, which OxyBrush doesn't do.

### Phase C: sculpt the splats

- **Bind to a mesh:** Splats > **Bind to piece** attaches each splat to
  the nearest triangle of a mesh piece (usually the one from Phase B):
  its position in the triangle and its height and rotation in the
  triangle's frame. Sculpting, posing or deforming the mesh then moves,
  turns and stretches the splats with it, so every brush, the gizmo and
  the Deform window work on a scan without new brush code. (Binding
  Gaussians to triangles: GaMeS, Waczyńska et al., 2024.) Unbind bakes
  the splats where they are. The mesh can be hidden while you sculpt.
- **Direct brushes** on unbound splats, for clean-up: Move, Smooth
  (positions), Erase (opacity), Paint (base color). A brush moves splat
  centres with the same falloff as on a mesh and turns and stretches each
  splat by the brush's local deformation, so splats don't tear.
- **Undo:** splats are stored in chunks like mesh points (invariant 2);
  edits snapshot only the touched chunks, and binding, unbinding, cropping
  and deleting are single undo steps.

### Phase D: Blender round trip

- **Send to OxyBrush** in the Blender add-on also sends splat objects:
  Blender has no built-in splat object, so add-ons store them as a point
  cloud or mesh points with the PLY properties as attributes; the add-on
  writes those as a splat PLY into the bridge folder, listed in the
  manifest with `"kind": "splats"`.
- **Send back** returns edited splats as a splat PLY and meshes as before
  (OBJ, keeping Blender's object when the point count is unchanged). The
  add-on updates the splat object's attributes in place when the splat
  count is unchanged, so the add-on that draws them keeps working, and
  otherwise replaces the object.
- **Meshes made from splats** go back as ordinary meshes with vertex
  color, so Blender can bake or texture them.
- Which Blender splat add-ons and attribute names to support is decided
  when Phase D starts, by testing the ones testers use; the bridge only
  reads and writes the standard PLY properties.

## Rules this keeps

- **Pure Rust, no new network access, no telemetry.**
- **Clean room.** The reference 3DGS implementation is under a
  non-commercial license and is not consulted; the work follows the
  published papers and file-format descriptions, logged in
  `docs/CLEAN_ROOM.md`. Permissively licensed splat renderers are not
  copied either; anything learned from their documentation is logged.
- **Never lose data.** Exports and `.oxb` writes stay atomic; an imported
  splat file is never overwritten.
- **GPU-free core.** Splat data, import, export, binding and Make mesh
  live in GPU-free crates (`oxy-doc`, `oxy-io`, `oxy-topo`); only the
  splat pass is in `oxy-render`.

## Not in this plan

- Capturing or training splats from photos or video.
- Relighting splats, or rendering them in Cycles or EEVEE.
- Animation of splats beyond following a bound mesh.

## Open questions

- Whether to keep view-dependent color (degrees 1 to 3) by default, or
  only base color, to save memory on large scans.
- The largest splat count to support on integrated GPUs (measured in
  Phase A).
- Whether Make mesh should also write a texture instead of vertex color,
  once UVs (RFC 0020) are in.
