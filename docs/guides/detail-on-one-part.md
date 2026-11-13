# Adding detail to one part of a model

You imported a model (an OBJ, for example) and it came in as **one piece**,
but its parts have very different resolutions: the head is dense, the
feathers on top are coarse. You want to sculpt texture into a few feathers
without subdividing the whole model, which would multiply every triangle
everywhere.

There are three ways to do it in OxyBrush. Pick the one that fits:

| | Best for | Result | Available |
|---|---|---|---|
| **A. Subdivide masked** | Quick extra resolution on a few parts | Triangles, finer only where masked | From 4.0.0-beta.9 |
| **B. Split the part into its own piece** | Clean quads, subdivision levels you can step through | Quads (with Quads chosen), full level workflow | Now |
| **C. Dynamic detail** | Adding resolution while you sculpt | Triangles, only where the brush goes | Now |

All three start the same way: pick out the part you want.

---

## Step 1: pick out one part

An imported model usually keeps each separate surface (each feather, each
eye, the head) as its own **island**: a set of faces not connected to the
rest. Turn the islands into face groups so you can select them:

1. Make sure the model is the active piece (**Alt+click** it if not).
2. **Groups > Group by islands.** Each separate surface gets its own face
   group.
3. Press **G** to see the groups in colour and check that each feather has
   its own colour. Press **G** again to go back to the normal material.

Now you can mask any part with one key:

- Point at a feather and press **Ctrl+M** (**Mask > Mask face group**). The
  whole feather is masked.
- Hold **Ctrl+M** over other feathers to add them too.
- **Alt+M** clears the mask; **Ctrl+Shift+I** inverts it.

> **If a feather shares a colour with the head**, it is welded to it
> (connected through shared points), so it isn't a separate island. Mask it
> by hand with the Mask brush instead, or use the Mask lasso.

---

## A. Subdivide masked (from 4.0.0-beta.9)

1. Mask the feathers you want to work on (Step 1).
2. Choose how the new points are placed: **Topology > Subdivision levels >
   Smooth**.
   - **On** (default): the new points follow the curve of the surface. Use
     this for organic shapes like feathers.
   - **Off**: the new points sit on the existing faces. The shape doesn't
     change at all. Use this for hard-surface parts.
3. **Mask > Subdivide masked** (also in the Mesh menu and the command
   palette, **Ctrl+K**).
   - Each masked triangle becomes four. The faces bordering the area are
     split just enough to keep the surface closed, so there are no cracks.
   - The rest of the model doesn't move, not even slightly.
4. Not fine enough yet? Run **Subdivide masked** again. The new faces are
   still masked, so each run makes them four times finer.
5. Protect everything else while you sculpt: **Mask > Invert mask**
   (**Ctrl+Shift+I**). Now only the feathers can be sculpted.
6. Sculpt your texture.
7. When you're done, **Alt+M** clears the mask.

Things to know:

- **Undo:** each Subdivide masked is one undo step (**Ctrl+Z**).
- **Edges become triangles:** the border of the subdivided area is made of
  triangles, even on a quad model. That's fine for sculpting; for clean
  quads use way B.
- **Subdivision levels:** like the other subdivide commands, it removes the
  piece's subdivision levels (undo brings them back).
- **Too many triangles?** If the status bar mentions the triangle limit,
  mask fewer parts at a time, or raise the limit in Preferences.

---

## B. Split the part into its own piece

This gives the part real **subdivision levels**: you can step down to the
original shape and back up to the detail.

1. **Pieces > Split into loose parts.** Every separate surface becomes its
   own piece; nothing moves. A detailed model can become dozens of pieces.
   - To split off only some parts, mask them (Step 1) and use **Mesh >
     Split masked** instead.
2. **Alt+click** a feather to make it the active piece.
3. Press **Ctrl+D** to add a subdivision level to that piece only.
   - With **Makes: Quads** (Topology > Subdivision levels, the default), a
     quad feather stays all quads, with its edge loops intact.
   - Turn **Smooth** off first if the part should keep its exact shape.
4. Sculpt. **D** and **Shift+D** step up and down between levels. Broad
   changes on a low level carry the fine texture along.
5. Repeat for each feather you want to detail.

Things to know:

- **Focus:** **Alt+S** (Pieces > Solo / show all) shows only the active
  piece, and **Alt+S** again shows everything. The eye in the Pieces list
  hides one piece.
- **Joining back:** **Pieces > Join visible** puts pieces back together
  exactly as they are, but it removes their subdivision levels. Join only
  when you're done, or keep them separate: they save in one project and
  export together.

---

## C. Dynamic detail while sculpting

1. **Topology > Add detail as you sculpt.** Set **Detail** to how fine you
   want it.
2. Mask everything except the feathers (Step 1, then **Ctrl+Shift+I**).
3. Sculpt on a feather. Triangles are added under the brush as you go; a
   smaller brush gives smaller triangles.

Things to know:

- **No subdivision levels:** dynamic detail only works on a piece that has
  none.
- **Uneven triangles:** the result is uneven triangles, which is good for
  sculpting but not for clean topology.

---

## Which one should I use?

- **"I just want to add texture to a few feathers, now."** Use A, or C if
  you're on 4.0.0-beta.8.
- **"I want clean quads and levels to go back to."** Use B.
- **"I want detail only where I happen to sculpt."** Use C.

Questions or problems: [report an issue](https://github.com/crisprintsstuff/OxyBrush/issues/new?template=bug_report.yml)
(or **Help > Report a bug…** in the app), or ask on
[Discord](https://discord.gg/Vfm7Km7xvG).
