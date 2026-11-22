# RFC 0032: To Circle in Poly Edit

Status: Proposed for the To Circle part of issue #370

## Summary

Pick points, edges or faces in Poly Edit, then click **To Circle**.
The selected points move toward one circle fitted to their shape. Use
**Amount** to blend toward the circle or apply the full fit.

## Why

Round openings and profiles often start as an irregular polygon or edge
loop. Fitting the picked points to a common radius lets artists round the
profile without placing each point individually.

## Design

- Points act directly. Picked edges contribute their ends, and picked
  faces contribute their corners. Duplicate points count once. Select
  the outline of an opening to round it.
- Fit a plane through the selection's centroid by minimizing squared
  distances to the plane. Fit the center in that plane.
  Use the mean projected radius and keep each point's projected direction. Amount blends the original 3D positions
  toward these targets. Three non-collinear points with distinct radial
  directions are required.
- Edit the explicit pick. The action keeps point and face indices,
  attributes, and the pick, and records one undo step. There is no file
  format change.

## References

The linked public manuals informed the selection workflow and blend
control. The fit uses ordinary least-squares plane fitting and radial
projection.

- [Blender To Circle](https://docs.blender.org/manual/en/5.4/modeling/meshes/editing/mesh/transform/to_circle.html)
- [Cinema 4D Fit Circle](https://help.maxon.net/c4d/en-us/Content/html/TOOLFITCIRCLE.html)

The issue's separate Knife request involves interactive path drawing and
topology changes. Its references are [Blender Knife](https://docs.blender.org/manual/en/2.81/modeling/meshes/editing/subdividing/knife.html)
and [Cinema 4D Line Cut](https://help.maxon.net/c4d/en-us/Content/html/TOOLKNIFELINE.html).
