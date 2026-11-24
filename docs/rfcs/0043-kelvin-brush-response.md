# RFC 0043 — Kelvin brush response

**Status:** First pass implemented. Pending hands-on sculpting across model scales and input devices.

## Brush behavior

| Brush | Gesture | Expected result |
|---|---|---|
| Kelvin Grab | Drag from the contact | The contact follows the drag at full strength. Neighbors move elastically, including beyond the cursor ring. Alt pulls along the surface normal through the same elastic field. |
| Kelvin Twist | Drag left or right | Twist more near the contact and fade toward the edge. The rotation axis follows the surface normal. Alt reverses the rotation. |
| Kelvin Scale | Drag up or down | Expand upward and contract downward around the contact. Alt reverses the response. |

Keep the stroke anchored at the original contact and evaluate deformation
from stroke-start positions. Returning the pointer to the starting position
restores the mesh. Preserve masks, hidden vertices, undo, and symmetry.
Reflected copies use the reflected rotation axis with opposite handedness.

## Field and interaction

Use the normalized translational Kelvinlet for Grab and the normalized
affine radial attenuation for Twist and Scale from de Goes and James
(2017). Use the brush radius for regularization. Use a smooth taper to
zero at twice that radius for bounded chunk queries. Keep the existing
large-drag damping of Grab's anisotropic response to reduce folds on
long pulls. Evaluate Twist as a finite rotation and Scale as a positive
exponential scale factor. These controls match the affine fields to
first order and keep a large drag from introducing radial stretch in
a single twist or reversing a scale through zero.

The published bi-scale and tri-scale Grab fields give tighter response
through cancellation of the distant terms. Blender's user manual exposes
these as separate locality choices, along with a volume preservation
control. Evaluate these as follow-ups once the default gestures and falloff
are verified. Time-dependent waves from Dynamic Kelvinlets belong to a
different workflow.

## Verification

- Compare light and long grabs on a sphere, a narrow horn, and a closed
  organic mesh. Check the contact, surrounding silhouette and folds.
- Try Twist and Scale from a head-on view. Check both directions, Alt,
  the contact, return to start, and a stroke across the mirror plane.
- Check masks, hidden vertices, pressure, and undo. Measure grab latency
  when the brush radius spans a large part of a dense mesh.

## Sources

- [de Goes and James, Regularized Kelvinlets, SIGGRAPH 2017](https://research.pixar.com/docs/2017.SiggraphPapers.GJ.pdf)
- [de Goes and James, Dynamic Kelvinlets, SIGGRAPH 2018](https://research.pixar.com/docs/2018.SiggraphPapers.GJ.pdf)
- [Blender user manual, Elastic Deform](https://docs.blender.org/manual/en/3.6/sculpt_paint/sculpting/tools/elastic_deform.html)
