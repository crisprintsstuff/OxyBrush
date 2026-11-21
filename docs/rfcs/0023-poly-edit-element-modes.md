# RFC 0023: Poly Edit element modes

Status: Complete (accepted 2026-11-15)

## Summary

Poly Edit ([RFC 0018](0018-poly-edit.md)) gets **element modes**, as
traditional modeling tools have: **Faces**, **Edges** or **Points**, picked
with **1**, **2**, **3** while the tool is open (and buttons at the top of
its window). **Auto**, key **4**, keeps the original behaviour, where the
element under the pointer decides. The window shows only the current
mode's actions, picking follows the mode, and Ctrl/Cmd+click picks several
elements of the current kind, which turn into the next kind when you switch
modes.

## Why

The maintainer: Poly Edit is becoming powerful, but every function is
exposed at once, as in ZBrush's polygon brush, and the panel is getting
cluttered. Traditional modeling tools separate faces, edges and vertices
(Blender: 1, 2, 3 on the keyboard, or buttons in the viewport).

Exposing everything at once also has costs beyond the panel:

- **Precision.** Auto has to guess from the pointer's distance to a point
  or an edge (8 points on screen). Near a small face's edge you get the
  edge when you meant the face, or the other way round.
- **Picks.** Ctrl/Cmd+click picking (#243) only made sense for faces:
  "what's under the pointer" is ambiguous for edges and points.

What was looked at: the maintainer's description of Blender's select
modes. No other product's docs or code were consulted for this RFC.

## Design

### Modes

| Key | Mode | A click picks | The window shows |
|---|---|---|---|
| 1 | **Faces** (default) | the face under the pointer, anywhere on it | Faces: actions, Reach, Each face on its own, bevel and bridge options |
| 2 | **Edges** | the nearest edge of the face under the pointer, however far | Edges: actions, edge Reach, Two loops, bevel and crease options |
| 3 | **Points** | the nearest corner of the face under the pointer | Points: actions, crease weight |
| 4 | **Auto** | as before: a point within 8 pt, else an edge within 8 pt, else the face | all three |

- The keys are the first four brush keys (Brush 1-4 in the keymap), taken
  over only while Poly Edit is open, so rebinding them moves both. Outside
  Poly Edit they pick brushes as always.
- Settings shared by every mode (Mirror, Smooth preview, Subdivide, Done)
  stay in the window in every mode.
- Points mode draws the points of the faces turned towards you as dots.

**Decision: Faces is the default** (the maintainer chose the recommended
option): it's the most common starting point and the smallest panel. Auto
stays for those who like the old way.

### Picks

- **Ctrl/Cmd+click** picks or unpicks an element of the current kind (in
  Auto, of the kind under the pointer). Only one kind is picked at a time.
  Ctrl/Cmd+click on empty space, or **Clear** in the window, unpicks all.
- Clicking or dragging a **picked** element applies the action to every
  picked element, instead of its Reach: extrude several faces, crease or
  bevel several edges, move several points. Mirror still adds the other
  side. Insert loop and Spin work on the edge clicked.
- Picks are by face and point numbers, so they end when the topology
  changes (after any action that adds or removes faces).
- Picked faces are tinted and outlined in green; picked edges are drawn
  thick in green; picked points as green dots.

**Decision: switching modes converts the pick** (the maintainer chose the
recommended option):

| From \ To | Faces | Edges | Points |
|---|---|---|---|
| Faces | | their sides | their corners |
| Edges | faces whose every side is picked | | their ends |
| Points | faces whose every corner is picked | edges with both ends picked | |

Switching to Auto keeps the pick as it is. So: pick a face, press 2, and
bevel its border edges; or pick a ring of points, press 1, and extrude the
faces they enclose.

## Phases

All three landed together:

1. Modes, keys, the window's sections and the picking per mode.
2. Picks for edges and points.
3. Converting the pick between modes.

## Not in this plan

- Box or lasso picking.
- Picks that survive a change of topology (they would need the action to
  report how it renumbered things).
- Showing every edge thicker in Edges mode: the edges are already drawn
  by the viewport, and the one under the pointer is highlighted.
