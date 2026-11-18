# RFC 0025: Editable strokes and portable detail

Status: accepted (decisions by the maintainer, 2026-11-16). Phases 1 and 2 are built (recipes, then Edit settings). Phases 3–6 are not.

## Summary

Two additions to Living strokes ([RFC 0017](0017-living-strokes.md)):

1. **Edit a stroke after it's made.** Change its brush, size, strength,
   falloff or alpha, or reshape its path, and the surface updates.
2. **Portable detail.** Save a group of strokes (a scar, a set of
   wrinkles, a row of scales) as a **detail** and place it on any model,
   where it's sculpted again on that surface.

Both come from one change: a stroke also keeps **what was drawn** (its
recipe), not only what it did to the surface.

## Why

Today a living stroke stores the change it made to each point. That is
enough to fade, delete, group, mask and mirror it, but not to change it: a
wrinkle made too wide has to be deleted and drawn again, and a stroke means
nothing on another mesh, because it is a list of that mesh's point numbers.
RFC 0017 left out "copy a stroke to another place" for that reason.

A recipe is independent of the mesh. With it, a stroke can be run again
with other settings, along another path, or on another surface. Sharing
brush presets shares tools; sharing details shares the work itself, which
other sculpting apps don't offer.

## What the user sees

### Editing a stroke

- **Edit…** on a stroke in the Strokes panel (or double-click its row)
  opens its settings: brush, Raise/Carve, size, strength, falloff, alpha,
  spacing. Changing one re-sculpts the stroke live.
- **Edit path** shows the stroke's path on the model as a curve with a few
  points, as the Curve stroke mode does. Drag points to move the stroke,
  add or remove points, and the stroke follows.
- Each change is one undo step. Esc cancels the edit.
- A stroke without a recipe says why it can't be edited: it was made
  before this version, with dynamic detail on, or by Curve Move or a
  whole-piece tool (Deform, the gizmo). Curve Move and those tools are
  not in the stroke list.

### Details

- **Save as detail…** on selected strokes or a group asks for a name and
  adds it to the **Details** list (a section of the Strokes window), with a
  small picture.
- **Place:** pick a detail and click a model. A preview is sculpted where
  you click, following the surface. Drag to move it, and use the handles to
  turn and scale it. Accept adds its strokes as living strokes, in a group
  named after the detail, as one undo step. They can then be faded and
  edited like any strokes.
- Mirror X/Y/Z places it on the other side too.
- **Export… / Import…** write and read a detail as a file
  (`.oxydetail`), to share it like a brush preset.

## How it works

### The recipe

`oxy_sculpt::Stroke` is already deterministic: the same samples, settings
and symmetry give the same dabs (its spray uses a per-stroke seeded
generator). The recipe is its input:

```text
StrokeRecipe {
    settings: BrushSettings,        // brush, size, strength, falloff, …
    invert: bool,
    symmetry: Symmetry, center: Vec3,
    alpha: Option<AlphaRef>,        // by a hash of the stored pixels, not the name
    view: Vec3,                     // camera forward, for screen-space brushes
    paint: Option<Vec3>,
    samples: Vec<StrokeSample>,     // position, normal, pressure
    spacing_scale: f32,             // see below
}
```

- **Spacing under load:** the dab budget widens spacing when dabs are slow,
  so the dabs depend on how fast the machine was. The recipe records the
  spacing scale each sample was applied with, and a replay uses it instead
  of measuring time.
- **Size:** a few hundred samples of 28 bytes, small next to the stroke's
  stored changes. It's kept on the stroke's record and saved in a new
  skippable `.oxb` section, `STRI`, after `STRK`. Older versions skip it
  and keep the strokes as they are today (fadeable, not editable).
- **Not recorded:** strokes made with dynamic detail on. The triangles
  may change under the stroke, so the recipe is left off whether or not
  this stroke happened to rebuild the mesh. When the mesh is rebuilt the
  stroke is baked with that topology change, as any stroke is; when it
  isn't, the stroke stays in the list and can be faded, without a recipe.
- **Not living strokes:** Curve Move and whole-piece tools (Deform, the
  gizmo). Curve stroke mode, Stamp, Line, and a freehand stroke kept
  when you answer "no" to "edit this stroke as a curve?" are recorded,
  including each curve point's size and strength.
- **Alpha identity** is a hash of every pixel quantized to the 16-bit
  values the project stores, not the alpha's name and not a sample of
  the in-memory floats (those change when a captured alpha is saved and
  opened). The recipe bytes are versioned (`OXSR` version 2); version 1
  is still read.

### Re-sculpting one stroke

To change stroke *k*:

1. Take stroke *k* and every later stroke off the surface (subtract their
   stored changes, a sparse add each).
2. Run the edited recipe through the brush engine on that surface.
3. The difference is stroke *k*'s new stored change.
4. Put the later strokes back, as they were stored.

This keeps RFC 0017's additive model: a later Smooth stroke keeps the
change it made and is not recomputed. It costs one stroke plus two sparse
adds per later stroke, so it stays interactive. Live dragging uses a
preview at reduced rate; the full result is applied when the drag ends.

**Exact replay of later strokes** (re-running every later stroke that has a
recipe, in order) is a separate, optional action, "Replay later strokes",
run as a background job with an estimate. It is not in the first phases.

Undo: an edit is a panel undo step holding the stroke's old recipe and old
stored change, as panel edits already do.

### The path as a curve

The samples are fitted with a handful of curve points (the same fit that
reopens the last stroke as a curve). Moving a point moves the curve; the
samples are resampled along it and projected back onto the surface without
stroke *k*, keeping each sample's pressure by its place along the path.

### Details: from one surface to another

A detail stores the recipes of its strokes relative to an **anchor**: the
first sample of its first stroke, with the surface normal there and the
stroke's starting direction as a frame. Each sample is kept as an offset in
that frame, in units of the detail's size, so the detail has no absolute
position or scale. Alphas are stored inside it.

Placing it at a clicked point with a turn and a scale:

1. Build the frame at the clicked point from its normal and the turn.
2. Map each sample's offset into that frame, then **project it onto the
   target surface** (along the frame's normal first, the nearest surface
   point if that misses), taking the normal there.
3. Scale brush sizes with the detail.
4. Replay the recipes in order on the target, each becoming a living
   stroke.

Because the strokes are sculpted again, the target can have any triangle
count or layout. Projection distorts a detail that wraps far round a
strongly curved surface (a band round an arm); a walk along the surface
(geodesic) would fix that and is a later improvement.

The detail's `.oxydetail` file is versioned like presets: a small header,
the recipes, and its alpha images. It holds only what the user drew. The
bundled starter details, if any, are made for OxyBrush (CC0).

### Rules kept

- No network: details are local files, shared by the user by hand.
- Everything is an undo step; `STRI` is a new skippable section, and
  existing sections don't change.
- GPU-free: recipes and replay live in `oxy-sculpt` / `oxy-doc`.
- Replay of a large stroke runs through `Editor::start_job` when the
  estimate is over a frame or two.

## Performance

- Recording adds one copy of the samples per stroke; nothing per dab.
- Re-sculpting costs about what making the stroke cost, plus sparse adds
  for later strokes. Numbers from `dab_timing` will be reported with the
  implementation.
- Placing a detail costs its strokes' dabs on the target. The preview
  replays at most a budgeted number of dabs per frame.

## Phases

1. **Recipes:** ✅ record and save them (`STRI`), with a test that replaying
   a recipe reproduces the stroke exactly. No visible change except a
   memory figure. Curve strokes, stamps, lines and a declined
   "edit as a curve?" are included. Dynamic detail is excluded whether
   or not the mesh was rebuilt. An alpha is identified by its stored
   pixels, so a capture still matches after save and open.
2. **Edit settings:** ✅ the Edit… panel, re-sculpting one stroke. Later strokes keep their stored change. A slider previews at a reduced rate and applies the full stroke when the drag ends; a long stroke runs off the UI thread. Esc cancels. A stroke with no recipe says why.
3. **Edit path:** the path as a curve.
4. **Details in a project:** Save as detail, place with move, turn and
   scale, mirror.
5. **Detail files:** export, import, the Details list across projects.
6. **Later:** geodesic mapping for details that wrap; Replay later
   strokes.

## Decisions

The maintainer chose the proposed option for each (2026-11-16):

- **Later strokes when one is edited** keep their stored change (the
  additive model, fast), with exact replay as a later, optional action,
  rather than always being replayed.
- **Recipes** are recorded whenever Living strokes is on (they're small),
  without a separate option.
- **Details live** in a Details section of the Strokes window, stored in
  the user's settings folder like presets, plus Export/Import.
- **Symmetric strokes in a detail:** a detail keeps one side only, and
  the target's Mirror setting decides whether it's doubled.

## Not included

- Details that change triangles (insert meshes, cuts).
- Strokes that survive remesh or subdivision (they could be replayed from
  recipes; a separate RFC).
- A shared online library of details.

[RFC 0026](0026-parts-bin.md) (draft) proposes the Parts Bin as the way
phases 4 and 5 are shown: a library strip, a drop onto the model, and a
placed detail that stays editable. It asks the maintainer to confirm
four amendments to the decisions above.
