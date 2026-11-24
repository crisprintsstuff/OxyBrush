# RFC 0043 — Kelvin brush response

**Status:** Grab blending and the Twist and Scale gesture changes are implemented. Pending hands-on sculpting across model scales and input devices.

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
control. The tuned Grab uses the Bi-scale field for short drags. Tri-scale and a separate volume control remain follow-ups. Time-dependent waves from Dynamic Kelvinlets belong to a
different workflow.

## Bi-scale Grab prototype

The prototype evaluates the paper's normalized two-radius field (equation
9), with regularization radii of 1 and 1.1 times the brush radius. The
comparison uses the same capture taper and long-drag damping for both
kernels. It runs both through the stroke engine on a synthetic horn blended
into a sphere, at full strength with a brush radius of 0.65 mesh units.

| Drag distance | Peak torso movement, current / Bi-scale | Sharpest horn-root angle, current / Bi-scale | Sharpest angle anywhere, current / Bi-scale |
|---|---|---|---|
| 0.32 | 0.0899 / 0.0358 | 9.6° / 10.5° | 15.0° / 17.4° |
| 0.65 | 0.1802 / 0.0688 | 11.9° / 15.4° | 17.3° / 24.8° |
| 1.20 | 0.3221 / 0.1099 | 10.3° / 20.8° | 23.7° / 39.9° |
| 2.00 | 0.5136 / 0.1451 | 11.8° / 23.5° | 36.1° / 61.2° |

Both kernels reach the same tip displacement. Bi-scale reduces peak torso
movement by 60–72% and produces a sharper transition as distance increases.
The tuned Grab blends in 55% of the Bi-scale field on drags up to half a
brush radius and smoothly returns to the single-scale field by one and a
half radii. These thresholds were tuned on the synthetic horn. The half-
radius pull reduced torso motion by 34% and increased the root angle from
9.6° to 10.1°. The one-radius pull reduced torso motion by 18% and increased
the root angle from 11.9° to 12.5°. Both longer benchmark pulls matched
the single-scale field. Production meshes and tablet feel remain to be
checked. The test runs with
`cargo test -p oxy-sculpt kelvin_biscale_horn_comparison -- --nocapture`.

## Verification

- Compare light and long grabs on a sphere, a narrow horn, and a closed
  organic mesh. Check the contact, surrounding silhouette and folds.
- Try Twist and Scale from a head-on view. Check both directions, Alt,
  the contact, return to start, and a stroke across the mirror plane.
- Check masks, hidden vertices, pressure, and undo. Measure grab latency
  when the brush radius spans a large part of a dense mesh.

## Sources

- [Pixar demo, multi-scale extrapolation](https://vimeo.com/216753419)

- [de Goes and James, Regularized Kelvinlets, SIGGRAPH 2017](https://research.pixar.com/docs/2017.SiggraphPapers.GJ.pdf)
- [de Goes and James, Dynamic Kelvinlets, SIGGRAPH 2018](https://research.pixar.com/docs/2018.SiggraphPapers.GJ.pdf)
- [Blender user manual, Elastic Deform](https://docs.blender.org/manual/en/3.6/sculpt_paint/sculpting/tools/elastic_deform.html)
