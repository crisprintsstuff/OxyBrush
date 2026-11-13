# What's coming in OxyBrush

A look ahead for testers. Nothing here is promised or dated: plans change as
we learn from you. The newest release is
[4.0.0-beta.7](https://github.com/crisprintsstuff/OxyBrush/releases/latest);
the detailed engineering plan is in the [roadmap](ROADMAP.md).

Tell us what you think, and what you'd want first, on
[Discord](https://discord.gg/Vfm7Km7xvG) or with **Help > Send feedback…**
in the app.

## The big one: Living strokes

> Every stroke stays editable, even after you've sculpted on top of it.

Today, once a stroke is done it's part of the model. If a wrinkle you made
twenty minutes ago is too deep, you either undo everything since, or sculpt
over it. Other sculpting apps work the same way: you can plan ahead with
layers, but a stroke you didn't put on a layer is final.

With **Living strokes**, OxyBrush remembers what each stroke did. A new
**Strokes** panel lists them, newest first, and for any stroke, without
undoing anything after it, you can:

- **Fade it:** drag its strength from 0 to 150% and watch the model change.
- **Delete it:** the stroke's change goes away; your later work stays.
- **Keep part of it:** mask the part you don't want and remove it from just
  that stroke.
- **See it alone:** hover a stroke to see where it touched the model; Solo
  shows only that stroke's effect.
- **Group strokes after the fact:** select a few strokes ("all the wrinkles"),
  give them a name, and fade or hide the whole group. It's a sculpt layer you
  didn't have to create in advance.

What to expect:

- **It's like layers, not time travel.** Fading a stroke adds or removes
  exactly what that stroke did. Strokes made on top of it keep what *they*
  did; they aren't redone. (An exact "redo everything after it" mode may come
  later for small models.)
- **Some tools set the strokes in stone.** Anything that rebuilds the
  model's triangles (voxel remesh, dynamic detail, decimate, booleans) bakes
  the stroke list first, and OxyBrush tells you when it does.
- **Strokes are saved with the project,** and older versions of OxyBrush still
  open the file.

It will arrive in steps: first the panel with fade, delete and solo, then
groups and saving, then extras like copying a stroke to the other side.
The full design is in [RFC 0017](rfcs/0017-living-strokes.md).

**Questions for you:**
- Which would you use most: fading, deleting, or grouping into layers?
- Should a stroke made with symmetry be one entry, or one per side?
- Would you rather have this fast and "like layers", or exact but slower on
  big models?

## Also planned

### Sculpting
- **Sculpt layers** you create ahead of time, with their own strength.
- **Stencils:** an image laid over the view that you sculpt through.
- **A brush editor and custom shelves** of your favourite brushes and presets.
- **A dedicated sharp-crease brush.**

### Models and topology
- **Rebuild lower subdivision levels** for a dense model imported from
  another app, so you can sculpt broad changes on it.
- **Faster dynamic detail on big models** (it's smooth up to a few hundred
  thousand triangles today).
- **Retopology:** quad strips and edge sliding.
- **Edge-preserving polish** for hard-surface work.

### Painting and materials
- **Materials per region:** different matcaps or shading for different parts
  of a model.

### Performance
- **Smooth sculpting at 10 million triangles,** measured on reference
  hardware.

### Platforms
- **iPad with Apple Pencil:** an iPad version, then palm rejection and
  pressure tuning, then a TestFlight beta. (Waiting on an Apple developer
  account.)
- **Signed releases:** a signed Windows installer and a notarized macOS app,
  so the security warnings on first launch go away.

### Working with other apps
- **More from the Blender bridge**, and more tools for local AI agents.

## Recently shipped (for context)

- **4.0.0-beta.7:** Poly Edit (first steps: extrude, inset, edge loops,
  close holes, weld), brush size on the model, vertex colors from OBJ and
  PLY imports, live size and position in the transform gizmo.
- **4.0.0-beta.6:** undo on the active piece only (Ctrl+Alt+Z), and
  reopening the last stroke as a curve, Curve Move curve or tube path.
  These are small first steps toward Living strokes.
- **4.0.0-beta.5:** the Deform window (bend, twist, taper, inflate, polish
  and 20 more), Mask border and Mask group borders, the Inter font.
- **4.0.0-beta.4:** Symmetrize, Split masked, Join, button and brush icon
  colors, five new plugins.
- **4.0.0-beta.3:** one Theme window, pen gestures for tablets.

The full list is in the [changelog](../CHANGELOG.md).
