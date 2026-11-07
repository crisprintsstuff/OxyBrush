# RFC 0004: Curve strokes you can adjust before applying

- **Status:** proposed for the maintainer; implemented in the same change for review
- **Requested by:** a user, after seeing a feature like this in another sculpting app.
  Only the idea was taken; no documentation, code or UI of that app was consulted
  (see [CLEAN_ROOM.md](../CLEAN_ROOM.md)).

## Problem

A long, even stroke (a seam, a wrinkle, the edge of a plate) is hard to draw
freehand. Line mode is straight, the stabilizer smooths but can't be
corrected afterwards, and undoing and redrawing loses the good parts of a
stroke along with the bad.

## Decision

A fourth stroke mode, **Curve**, next to Freehand, Line and Stamp.

1. **Draw** the stroke as usual. On release it becomes a smooth curve
   through a few control points (the drawn path simplified with
   Ramer-Douglas-Peucker, at most 16 points) instead of being applied.
2. **Adjust** it while it is open:
   - drag a control point to reshape the curve (it stays on the surface);
   - click the curve between two points to add one there;
   - Alt+click a point to remove it (at least two remain);
   - the brush's Size, Strength and other settings, and **Fade in** /
     **Fade out** (how much of the length, from each end, the stroke takes
     to reach full strength), update the result.
3. **Apply** with Accept or Enter, or by starting another stroke on the
   model. **Cancel** or Esc discards it. Orbiting, panning and zooming
   leave it open, so you can look at it from another side.

The result is shown live, on the model, while it is open.

## How it works

- **Preview without touching the document.** The open curve keeps a copy of
  the piece and applies the stroke to it; the renderer draws the copy in the
  piece's place. Each change restores only the chunks the last build
  touched, then applies the stroke again, so a rebuild costs about as much
  as the stroke itself. The piece and the undo history are unchanged until
  the curve is applied.
- **Exact apply.** Strokes are deterministic (spray uses a fixed per-stroke
  seed), so Accept replays the same samples on the piece through the normal
  `History::begin`/`touch`/`commit` path. The result is identical to the
  preview and is one undo step (architecture invariants 2 and 4).
- **View-independent.** Control points are surface points with normals, and
  the curve is a Catmull-Rom spline through them in world space. Samples are
  snapped back to the surface along the curve's normal, not cast from the
  camera, and the brush size is fixed in world units when the curve is
  drawn, so orbiting or zooming doesn't change the stroke.
- **Safe when something else happens.** If the piece's history revision or
  topology changes while the curve is open (undo, a remesh, another tool) or
  another piece is selected, the curve is dropped. The piece was never
  modified, so nothing needs restoring.
- Symmetry, the mask, alphas and every brush that supports Line mode work
  as in a normal stroke, because the same stroke engine applies the samples.
  Moving brushes (Grab, Snake Hook) and the Shift/Ctrl stand-ins stay
  freehand, as they do for Line and Stamp.

## Not in this change

- Saving an open curve with the project, or editing a curve after it has
  been applied.
- Per-point size or strength (only the two end fades).
- Closed curves (loops).
- Snapping to edges or face-group borders.

## Alternatives considered

- **Screen-space curve, re-cast from the camera each rebuild** (as Line mode
  does): simpler, but the stroke would change when you orbit to inspect it.
- **Preview by applying to the real piece and rolling back with history
  snapshots:** avoids the copy, but every other editor operation would have
  to know about the half-recorded stroke. The copy keeps the document and
  history untouched, so they can never get out of step.
