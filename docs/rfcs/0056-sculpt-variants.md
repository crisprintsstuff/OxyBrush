# RFC 0056 — OxyMorph

- **Status:** Beta. Native forking, live blending, paintable pins, and linked comparison implemented.
- **Scope:** Try two sculpt directions on one piece and paint a third version from them.

## 1. Concept

A sculptor wants the brow from one creature head and the jaw from another.
Both began with the same head. They must decide where to keep each change
and how the forms meet. The originals are still useful as those decisions
evolve.

We propose three linked views of one piece. Sculpt independently in A and B
and see the result between them. Paint **Keep A**, **Keep B**, or **Erase**
on the result. Turn the model once to see the same angle in all three
views. Continue sculpting either version after painting. The result
updates in place, with the painted choices still attached to the surface.

The trial uses an original clay head with broad and crested versions.
Drag on either side to change its shape. Paint the forehead and jaw with
different mix brushes, then move **Global mix**. Areas strongly painted
toward a version should stay there. Export the result as an OBJ to inspect
it in OxyBrush. [Open the prototype](../prototypes/sculpt-variants/index.html)

The prototype runs offline in a browser. From the repository root, serve it
locally with the command below and visit `http://127.0.0.1:8769` if your
browser requires HTTP for local pages.

```sh
python3 -m http.server 8769 --bind 127.0.0.1 --directory docs/prototypes/sculpt-variants
```

Run the geometry and mix tests with
`node --test docs/prototypes/sculpt-variants/model.test.js`.

## 2. Related workflows

| Application | Closest documented workflow |
|---|---|
| SculptForge | The timeline keeps alternate history branches that users can switch between. Named sets store shapes. Its Morph brush pulls local geometry toward a stored version, and Morph to Set interpolates globally while respecting a mask. The change can go into an active sculpt layer. Weighted sculpt layers export as blend shapes. [Selection sets and morph targets](https://sculptforge.app/guide/selection-sets.html#morph-targets) · [Sculpt timeline](https://sculptforge.app/guide/timeline.html) · [Sculpt layers](https://sculptforge.app/guide/layers.html) |
| ZBrush | A morph target stores a sculpt state, and the Morph Brush restores it locally. 3D Layers separately record edits with intensity controls. Maxon documents a workflow using both to erase part of a layer. [Morph targets](https://help.maxon.net/zbr/en-us/Content/html/user-guide/3d-modeling/sculpting/morph-targets/morph-targets.html) · [3D Layers](https://help.maxon.net/zbr/en-us/Content/html/user-guide/3d-modeling/sculpting/3d-layers/3d-layers.html) |
| Nomad Sculpt | Additive sculpt layers support adjustable weights, partial erasure, and conversion between matching objects and layers. Nomad also transfers layers through several topology operations. [Layers](https://nomadsculpt.com/manual/layers) |
| 3DCoat | The Sculpt Tree organizes surface and voxel objects and supports duplication and composition. [Sculpt Tree](https://3dcoat.com/documentation/manual/workspaces-rooms/sculpt/sculpt-tree/) |
| Adobe Substance 3D Modeler | Clay tools, symmetry, repetition, and booleans support exploring and assembling forms. [Modeler features](https://www.adobe.com/products/substance3d/apps/modeler.html) |
| Blender | Shape keys store alternate vertex positions. Relative keys support weights and vertex-group restrictions, as documented in SculptForge’s bridge guide. [Shape key interoperability](https://sculptforge.app/guide/blender.html) |

Local morphing, alternate histories, masks, and weighted layers already exist. The design
opportunity is a dedicated sculpting session that combines two editable
endpoints, linked comparison, and a reusable surface field that chooses
between them. I did not find that complete workflow documented in the
tool pages reviewed. This is a comparison of public documentation as of
November 27, 2026.

OxyBrush’s split view and editable stroke groups provide native building
blocks for the feature.
[Split view](0054-split-view.md) · [Living strokes](0017-living-strokes.md)

## 3. Native implementation

### First slice

- Open **Tools > OxyMorph (Beta)** on the displayed mesh of a sculptable piece to
  use it as A. **Fork B** makes an ordinary editable piece with matching
  vertex identity and selects it. Select A or B to keep sculpting in the
  same viewport.
- Turn on **Preview mix** and adjust the global mix. The preview temporarily
  replaces both input surfaces in the viewport without editing either
  piece or changing their visibility settings. Drag to orbit the preview.
- **Create piece from mix** adds a regular sculptable result and records one
  Undo step. Closing the panel discards the preview. The editable fork is
  a normal piece with its own Undo step.
- Blend only forks that share the original topology version. A topology
  change or subdivision level switch invalidates the blend. Undoing the
  change can recover correspondence. The initial limit is 200,000 triangles
  at the displayed level. Mix positions and keep A's material and mesh
  attributes. Saved projects retain ordinary pieces. The in-progress
  session and fork correspondence are scoped to the current project
  session.

### Painted mix

- In **Preview mix**, choose **Keep A** or **Keep B** and brush on the
  displayed surface. At full strength the center of the brush pins to that
  variant. Softer areas still respond to the mix slider. **Erase** fades
  pins back to slider control.
- **Show paint map** displays A's pins in blue, B's in orange, and unpainted
  areas in gray. The map and sculpt masks are independent. Orbit by dragging
  off the mesh or choosing **Orbit**.
- Undo and redo step through the last 32 whole paint strokes while the preview is on.
  Escape cancels the current stroke. Turn off the preview or select A or B
  to sculpt a source or undo source edits. Preview the pair again to see
  both the latest source shapes and the retained pins. Choosing a different
  B or closing the panel starts with a blank map the next time.
- **Create piece from mix** records one scene Undo step. The piece keeps A's
  material, color, and mesh attributes.

### Linked comparison

- Choose **Compare A / Mix / B** in the OxyMorph panel. A and B are shown
  in separate panes even if one is hidden in the scene. Orbit, pan, zoom,
  and frame use one camera so all panes have the same angle and scale.
- Sculpt in the side panes using the regular topology-preserving brushes
  with **Freehand**, **Line**, or **Stamp**. Dynamic detail is paused during
  comparison to keep the variants matched. For Curve strokes and
  topology-changing tools, turn off comparison first.
- Paint the live mix in the center. Undo and Redo use paint history when
  the center is focused and regular project history in the outer panes.
  The **Undo paint** and **Redo paint** buttons work from any pane.
- Turn off comparison to return to the previous viewport layout.

### Follow-up scope

- Persist in-progress sessions with projects.
- Limit the first pass to positions. Starting on dense imported meshes,
  mixing paint/materials, comparing different topology, changing
  subdivision levels within a session, and exporting animated blends need
  separate validation.

## 4. Model

For each keyed vertex, retain two nonnegative painted weights, `keep_a`
and `keep_b`, with a sum at most one. A dab fades both existing weights by
its falloff and adds that portion to the chosen side. Erasing fades both
toward zero. This leaves room for the global slider `g` on unpainted areas.

```text
weight_b = keep_b + (1 - keep_a - keep_b) * g
result   = position_a + (position_b - position_a) * weight_b
```

Use the same vertex identity across A, B, the mix, and the paint field.
Pick on displayed geometry and walk visible triangle edges within the
brush radius. A nearby fold is reachable only if the path along the
surface fits within the brush radius.

Validate the shared topology version before showing the blend or creating
the result. A global slider change can move every point. Paint updates the
vertices reached by the brush and the affected mesh chunks. Strongly
opposing edits can self-intersect during interpolation.
Test that explicitly before defining any automatic guard.

## 5. Review and acceptance

- An artist can keep A's cheek and B's crest, then sculpt A again and see
  the cheek update through the existing paint field.
- Moving the global slider leaves fully painted vertices pinned and
  interpolates unpainted vertices between the exact A and B endpoints.
- Painting the mix leaves both endpoint meshes unchanged. Erase restores
  the slider's control, and Undo restores one gesture.
- The exported mix opens as one closed mesh with the same face indices.
  Native Apply and Cancel preserve source geometry and honor shared Undo.
- Artist review should determine whether three panes earn their space, how
  useful it is to paint on the result, and whether a source or result
  view makes the most natural default target.
