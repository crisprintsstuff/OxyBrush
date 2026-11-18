# RFC 0026: Parts Bin

Status: Accepted. On 2026-11-18 the owner confirmed the four RFC 0025
amendments below, as well as scale, per-side fade and delete, refusing a
drop while dynamic detail is on, the symmetry-plane prompt, and the
remaining decisions recorded at the end.

## Summary

A **Parts Bin**: a library of saved details (a set of eyes, an ear, an
arm) that you drag from a strip along the bottom of the viewport onto a
model. On drop you choose a mirrored pair or a single copy. The part is
sculpted again at that spot from its stroke recipes, and it stays fully
editable afterwards as living strokes until an operation bakes them.

This builds on [RFC 0025](0025-editable-and-portable-strokes.md) phases 4
and 5. A part **is** a detail from that RFC, saved as `.oxydetail`. The
bin adds the library on disk, the strip, the drag, the choice at drop,
and the thumbnail. It does not add a second file format or a second way
to re-sculpt.

A part is not an inserted mesh: it has no vertices, topology or
subdivision stack of its own. An ear or arm made this way is sculpted
from strokes on the existing surface; it is not separate geometry joined
to the piece.

## Why

RFC 0025 separates a stroke's recipe from the mesh it was drawn on, so a
group of strokes can be placed on another model. What it does not yet
give you is a library you keep across projects and a way to drop a part
onto the surface in the view, then keep working on it as strokes.

The owner's sketch shows that library as a horizontal strip titled
**Parts Bin**, between the brush list and the settings panel, with
square thumbnail slots, a slot that opens to the part or its variations,
and an arrow at the right that pages to more slots.

## Relationship to RFC 0025

Use these as they stand:

- **Phases 1 and 2 are built.** A living stroke can carry a
  `StrokeRecipe` (`STRI`). **Edit…** re-sculpts that stroke through
  `resculpt`. Later strokes keep the change they stored (the additive
  model). A stroke with no recipe says why.
- **Phase 4's detail.** Recipes relative to an anchor (the first sample
  of the first stroke, its normal, and the stroke's starting direction).
  Samples stored as offsets in that frame, in units of the detail's
  size, so the file has no position on a mesh. Alphas stored inside.
  One side of a symmetric stroke only. Placing maps the offsets into a
  frame on the target, projects them onto the surface, scales brush
  sizes with the detail, and replays the recipes in order as living
  strokes in a group named after the detail.
- **Phase 5's file.** `.oxydetail` is versioned like a brush preset: a
  small header, the recipes, and the alpha images. It holds only what
  was drawn. Sharing is a file the user copies by hand. No network.
- **Phase 6, later.** A walk along the surface (geodesic) for a detail
  that wraps, and the optional "Replay later strokes". This RFC does
  not define that walk. Placement uses the projection until phase 6
  exists, then uses the walk for the same parts.

The following four amendments to RFC 0025 were confirmed by the owner on
2026-11-18 and are recorded in Decisions below.

1. **Where the library is shown.** One shared library is shown as the
   Parts Bin strip, not a separate Details list. The Strokes window keeps
   **Save as part…** and lists the placed strokes as a group.
2. **Who chooses the mirror.** The file stores one side. The drop asks
   **Place one** or **Mirror pair**; the piece's Mirror toggles only
   supply the default axis.
3. **When a place is committed.** The drop commits as one undo step,
   with move, turn and scale available afterwards, rather than a modal
   Accept that ends placement.
4. **A mirrored copy's recipe.** Mirror copy today stores no recipe
   (`OXNR` tag 2): Edit… tells you to edit the original. A mirrored
   **part** stores a recipe on both sides, so both stay editable, and
   the sides stay linked until you unlink. Mirror copy of an ordinary
   stroke is unchanged.

## What you see

The strip sits on the bottom edge of the viewport, in the gap between
the brush shelf and the settings panel. It stays on the viewport when
the shelf is docked at the bottom, above that shelf, so the two do not
share a row. It is one row of square slots. **View > Parts Bin** hides
it. Hiding the panels hides it too. The Touch layout uses the same row
with larger slots. No new shortcut, so the shortcut list is unchanged
until one is added.

Each slot is a thumbnail and the part's name. The right-hand arrow
shows the next page of slots. From the second page, a left-hand arrow
goes back. A category menu at the left of the strip filters the row
(**All**, then each category you have set). An empty bin is one slot:
"No parts yet. In Strokes, select strokes and choose Save as part."

Clicking a slot opens it upward: a larger thumbnail, the name, and the
other parts in the same category as further squares (its variations).
Rename, delete, export, category and reorder live in that panel. A drag
that starts on a slot or a variation is a placement. A drag that ends
on another slot does not reorder; reorder is in the panel, so it is not
confused with a drop.

**Save as part…** is on the selection in the Strokes window. It asks
for a name and a category, writes the `.oxydetail` into the library,
and makes the thumbnail. Strokes that cannot be a part are left out,
with the reason, in the wording under "What cannot be a part".

## A placed part stays editable

This is a requirement of the feature. A drop that left strokes you
could not edit would not be a part.

The drop adds a stroke group on the piece that was hit, named after the
part. Every stroke in the group keeps a `StrokeRecipe`, saved in `STRI`
with any other recipe. Living strokes has to be on. With it off, the
strip and **Save as part…** say: "Living strokes is off. Turn it on in
Preferences > General to save or place a part."

### In the Strokes window

The group is an ordinary stroke group, plus the placement record below.

- **Edit…** (or double-click) changes that stroke's brush, Raise or
  Carve, size, strength, falloff, alpha or spacing, and re-sculpts it.
  This is RFC 0025 phase 2, on the placed recipe.
- **Edit path**, when RFC 0025 phase 3 ships, shows that stroke's path
  as a curve. A placed stroke is a normal recipe, so it uses that
  editor. This RFC does not add a second one.
- **Fade** is the group's strength. **Solo** shows the group. **Delete**
  removes the group's strokes and its placement record, in one undo
  step.
- **Mirror**, on a part that was placed as one side, adds the other
  side as a linked pair, in one undo step. On a pair that is already
  linked, the control reads that it is already mirrored.
- **Move on surface** moves, turns or scales the whole part. The anchor
  slides on the surface, the twist handle turns it around the normal,
  and scale uses the same rules as the drop. Letting go re-sculpts the
  part and records one undo step. Esc puts it back and records nothing.

Fade, solo, delete and Mirror of strokes that are not a part stay as
they are in [RFC 0017](0017-living-strokes.md).

### Linked pairs

A mirrored drop writes both sides into the same group and marks the
group linked. Each side has its own recipes. The mirror side's samples
are the source samples reflected through the piece's symmetry plane.
Both recipes are replayed with symmetry off, so the brush engine does
not mirror them again. A mirrored pair is linked by default, so edits
propagate in either direction. The user can choose **Unlink** to turn
off that propagation; undo of Unlink restores it.

The side placed from the bin is the **source side**, even when it is on
the negative side of the axis. The other is the **mirror side**. These
roles follow the placements, not their current coordinates, and survive
moving, unlinking, saving and reopening. When Mirror is added to a
single placed part, that existing placement is the source. For
**Mirror, halves match**, the half placed before reflection is the
source. An edit initiated on the mirror side updates the source side
while linked; it does not make the mirror side the source.

While the link is on, an edit on either side is applied to the other
in the same undo step:

- **Edit…** copies the setting to the partner stroke and re-sculpts
  both.
- **Edit path** mirrors the new path through the plane and re-sculpts
  the partner.
- **Move on surface** moves, turns and scales the part. The other side
  stays the mirror, so the two sides do not come apart. Ending that
  move within the symmetry-plane tolerance asks the same prompt as a
  drop there. Cancel puts the part back where the move started.

Fade and delete of a stroke apply to that side only. The link does
not copy them. The link covers **Edit…**, **Edit path**, and move,
turn and scale. It does not cover the strength slider or Delete on a
stroke row.

The Strokes window shows one group, named after the part. While the
link is on, the group row says Linked. The group's strength, Solo and
Delete cover both sides, because they act on the whole group. Each
stroke row has its own strength slider and Delete, and those change
that stroke only. **Edit…** on a row says: "This change also applies
to the matching stroke on the other side. Fade and delete do not."

Fading one side to zero hides that side and leaves the other in
place. The link stays, so a later **Edit…** still updates the hidden
side's recipe. Deleting the last stroke on one side clears the link,
and the status line reads "The other side was deleted, so this part
is no longer linked."

**Unlink** is the user's choice to stop linked edits; it clears the
link flag and does not move the mesh. **Edit…**,
**Edit path**, and **Move on surface** then affect the side you
change. Fade and delete of a stroke were already per side. The two
placement records still name each other. The group stays one group,
so its strength slider still covers every stroke in it. Undo of
unlink restores the flag. Nothing links them again on its own. A
command to link them again is not in these phases.

### The bin entry does not change on its own

An edit, a move, a fade or a delete changes the piece. The library
file is written only when you ask:

- **Save back to bin** asks "Replace *name* in the Parts Bin with this
  version?" and then replaces that `.oxydetail` with one side's current
  recipes, in anchor-relative form, and regenerates the thumbnail.
- **Save as new part** asks for a name and adds a new file.

A linked pair saves one side, which is all a part file stores. If the
library file has been deleted or renamed out of reach, **Save back to
bin** offers **Save as new part** instead. Closing the project, saving
the `.oxb`, or editing the part does not write the library.

For a pair, **Save back to bin** uses the source side's current recipes,
even if the last edit was made on the mirror side or the pair was
unlinked. It keeps the original bin entry's handedness, not whichever
side happens to be positive. If the entire source side was deleted, the
command refuses to replace the entry and explains why; it does not
silently substitute the mirror side. The user can instead explicitly
save the remaining side as a new part. Per-side deletes and fades can
make the two sides different; saving back does not combine them. The
source side's remaining recipes are saved, including their brush
settings, but living-stroke and group fade multipliers are not part of
the recipe file. If either multiplier differs from 100%, the
confirmation warns that the fade will not be saved. Deleting a
source-side stroke is intentional and omits that recipe; a missing
recipe due to baking is not treated as an intentional deletion.

Save in the source placement's anchor frame, preserving its orientation
rather than reflecting it to the positive axis. Asymmetric targets can
still make the two replayed results look different; this rule preserves
the recipes' handedness, not a guarantee of identical surfaces.

The file contains recipes and alphas, not the piece's mesh, its active
level or any subdivision levels. The thumbnail is only a picture. On a
later placement the recipes are replayed on the new target's active
level, so the result can vary with its surface and resolution. Later
sculpting outside the part's stroke group is not captured by saving its
recipes.

Save back requires the source side's recipes to still be editable. A
subdivision change or another bake can remove them; in that case the
command cannot reconstruct them from the visible surface and must not
overwrite the bin entry with an incomplete part. Saving the project
keeps the baked surface, but does not restore those recipes. Save back
before the level change if the edited recipes are needed in the bin.

### Later strokes, and the additive model

RFC 0025's rule is unchanged. A later stroke keeps the change it
stored. It is not recomputed.

- **Edit…** on one stroke of the part uses `resculpt`. That stroke and
  every stroke made after it come off, the edited recipe runs, and the
  later strokes go back on as stored. The linked partner is
  re-sculpted in that same undo step, so the link is not left stale.
- **Move on surface** takes off every stroke of the part, and every
  stroke made after the earliest stroke of the part. It maps the
  part's recipes to the new frame and replays them in their original
  order, so the part's own strokes stack as they did. Strokes that are
  not in the part, and that were made after it, go back on unchanged.
  A wrinkle sculpted on the eye after the drop stays where it was when
  the eye moves. When any such stroke exists, the status line reads
  "Strokes made after this part stay as they were."
- **Replay later strokes** stays the optional action RFC 0025 defers.
  Moving a part does not turn it on.

### When it stops being editable

The events that bake any living stroke bake a part: remesh, decimate,
a subdivision change (including switching levels), dynamic detail
rebuilding the mesh, booleans, and the other topology changes in RFC
0017. The Strokes window uses the note it already shows.

The drop itself must not bake the strokes it just added. A drop while
dynamic detail is on is refused, as decided below. Placed part strokes
use a separate Parts memory pool, with a user-adjustable limit, rather
than the ordinary living-strokes budget. If a placement would exceed
the Parts memory limit, the drop is refused without changing the mesh.
The status line says: "Parts memory limit reached. Raise the limit in
Preferences or remove placed parts to free space."

## How it works

### A part is a detail

Saving copies the chosen recipes and their alphas into a `.oxydetail`,
using RFC 0025's anchor. The size unit is the first saved stroke's
brush radius. Offsets in the file are the sample positions in the
anchor frame, divided by that radius. Brush radii are stored as drawn
and multiplied by the placement scale. The recipe bytes stay
`StrokeRecipe` (`OXSR`), including each sample's spacing plan, so a
replay does not depend on how fast the machine is.

The byte layout of `.oxydetail` is the one phase 5 of RFC 0025 fixes
when it is implemented: a versioned header, the recipes, and the alpha
images. Parts Bin adds no required field. Order, category and a
content hash live in the library index, not in the detail, so a shared
file stays a detail. An optional skippable thumbnail chunk can be
added later; readers skip chunks they do not know, as `.oxb` readers
do. The first version does not need that chunk.

Symmetric strokes are reduced to one side at save time, as RFC 0025
decided. Samples on the positive side of the piece's mirror axis,
through its symmetry centre, are kept, including samples on the plane.
The recipe's symmetry flags are cleared. If several of Mirror X/Y/Z
were on, the positive-X side is kept. A stroke with radial symmetry is
left out of the part (see below).

### Where the library is stored

The library is a folder of files next to the brush presets, under the
app's data directory. The `dirs` crate's local data directory, and the
same app folder name the presets use (`OxyBrush` on Windows,
`oxybrush` elsewhere):

| System | Folder |
|---|---|
| Windows | `%LOCALAPPDATA%\OxyBrush\parts\` |
| Linux | `$XDG_DATA_HOME/oxybrush/parts`, or `~/.local/share/oxybrush/parts` |
| macOS | `~/Library/Application Support/oxybrush/parts` |

Settings stay in the config directory (`settings.txt`). The Parts Bin
uses the app's data directory, beside brush presets, as decided below.

Nothing in the folder is fetched or uploaded. The updater and the
local MCP server are unchanged.

Each part is `<name>.oxydetail`. The thumbnail is `<name>.png` beside
it, a cache. `library.txt` records order, category and a hash of the
file bytes:

```text
# OxyBrush parts library. Deleting it keeps the parts and loses order and categories.
version = 1
order = eyes.oxydetail, ears.oxydetail
category.eyes.oxydetail = Eyes
hash.eyes.oxydetail = <SHA-256 of the file bytes, hex>
```

File names follow the brush-preset rule: letters, digits, spaces, `-`
and `_`, trimmed, at most 80 characters, and a name that would leave
the folder is rewritten. Every write is a temporary file in the same
folder, then a rename. A file that is not a readable `.oxydetail` is
skipped. On open, a detail that is missing from `order` is appended,
so a crash mid-rename is recoverable. Deleting a library part does not
change a project that already placed it.

### Placement

A drag starts on a slot. The thumbnail follows the pointer. While the
pointer is over the mesh, a preview is sculpted (below). Releasing
over empty space, or Esc, restores the mesh and records no undo step.
Solo on, or a background job already running, refuses the drag with a
line in the status bar.

The hit uses the same ray the brushes use, on the piece under the
pointer, skipping hidden faces. That piece receives the strokes and
becomes the active piece. A miss, or a hit that the mask blocks
completely, places nothing: "The mask blocked this part, so nothing
was placed."

On release over the surface, a choice sits at the pointer. The single
preview stays up until you choose.

- **Place one.**
- **Mirror pair**, named with its axis ("Mirror pair, X") when exactly
  one of Mirror X/Y/Z is on. If none or several are on, the choice
  lists X, Y and Z, and X is the default.

Hovering **Mirror pair** previews the second side. Choosing either one
commits. Esc cancels. If the drop is within the symmetry-plane
tolerance below, this choice is replaced by the on-plane prompt. Esc
and Cancel there cancel the drop.

The replay, after the preview is reverted:

1. Build the frame at the drop point from the surface normal and the
   twist.
2. Map each sample's offset into that frame, at the chosen scale, then
   project it onto the target along the frame's normal. If that misses,
   use the nearest surface point, and take the normal there. This is
   RFC 0025's mapping.
3. Multiply each brush radius by the scale.
4. Clear symmetry on the recipe (mirror axes and radial count),
   whatever the original recipe stored and whatever Mirror is set to
   on the piece.
5. Replay the recipes in order with `StrokeRecipe::replay`, each
   becoming a living stroke with that mapped recipe. A long replay
   runs in `Editor::start_job`, as an edit already does when
   `replay_estimate_ms` is over a frame or two (32 ms).
6. Put the strokes in one group named after the part, and write the
   placement record.

The geodesic walk from RFC 0025 phase 6 replaces step 2 when it
exists. Until then, a part that wraps far round a curved surface is
stretched by the projection. The status line says so (below). The
walk is not specified again here.

### Scale, twist, and the live preview

The default scale follows the current brush, not the size recorded in
the file. The reference radius is the first saved stroke's brush
radius, and it is the detail's size unit: offsets in the file are
divided by it. On place that radius is drawn at the brush's current
world radius `R`:

```text
scale        = R / reference radius
world offset = stored offset × R
new radius   = recorded radius × scale
```

A stroke that was twice the reference radius lands at twice the
current brush. The scale handle, a drag away from the anchor in the
tangent plane, edits the factor after that start. **Original size**
on the drop choice sets the scale to 1, which is the recorded size.
It is available, and it is not the default. If the reference radius
is missing or not positive, the scale is 1. **Move on surface**
starts from the scale the part was placed at, not from the brush
again.

**Size: On the model** changes which world radius `R` is. It does not
turn the default off.

- **On the model** (Brush settings > Size > On the model) stores the
  brush as a size on the model, in scene units. `R` is that size.
  Zooming during the drag does not change the part.
- **On screen** is the default. The brush is a size on screen, and
  `R` is that size converted to world units at the surface under the
  pointer, the same conversion a stroke uses. Zooming in makes the
  brush cover less of the model, so the part gets smaller. The
  preview uses the world radius at the pointer, and the part tracks
  the brush while you drag.

The scale written into `PART` is the one at commit. Afterwards the
part does not keep following the brush.

**Twist** is a rotation around the surface normal. The part's recorded
starting direction, projected into the tangent plane, maps to the
twist direction. At the start of a drag that direction is a stable
world axis projected into the tangent plane, with a deterministic
alternate if it is nearly along the normal. It does not depend on the
camera. The twist handle orbits it. The committed frame is stored with
the placement.

The preview matches **Edit…**. `replay_estimate_ms` is about half a
millisecond a dab. At or under 32 ms, the preview runs the real
recipes. Over that, spacing is widened by the same factor as
`preview_recipe` (clamped from 1 to 8), and another preview does not
start until 50 ms or 1.5 times the last preview's cost has passed,
whichever is longer. The committed recipe is the full one. The preview
is drawn by sculpting and restoring, as an Edit… drag does, and it is
not an undo step.

### Curvature, small meshes, dynamic detail, subdivision

After projection, compare each stroke's mapped length with its
recorded length. If any stroke differs by more than 15%, the status
line reads "This surface curves more than the part was drawn on, so
the part is stretched." The drop is still allowed. Phase 6 of RFC 0025
is the fix.

A sample whose projection misses, and whose nearest point is farther
than the part's extent, is omitted and counted: "12 points didn't
reach the surface." If fewer than half the part's samples land, the
drop is refused and the mesh is unchanged: "This part doesn't fit on
the surface here. Scale it down, or drop it on a larger surface." The
same sentence covers a mesh smaller than the part. The scale is never
changed silently to make it fit.

**Dynamic detail.** Strokes made with it on have no recipe, whether or
not that stroke rebuilt the mesh, so they cannot be saved. That is
the phase 1 and 2 rule: dynamic detail changes the triangles, so the
stroke cannot be edited. A drop while it is on is refused for the
same reason, and the mesh is unchanged. The status line reads
"Dynamic detail is on, which changes the triangles, so this part
can't be placed. Turn it off." The drop does not turn it off for
you. Later, turning it on and refining can still bake the part, and
the existing bake note says so.

**Subdivision levels.** The piece, not the part, owns the subdivision
stack. The part is replayed on the active level only. The drop does not
add a level or change triangles. The piece's multiresolution machinery
syncs its levels as it does for other sculpting; this is not a mapping
of a part's base and detail levels onto the piece, nor a guarantee that
the part's detail can be represented at every level.

Before committing the drop, compare the part's smallest scaled brush
radius with the local mean edge length at the drop on each existing
level, using the surface before the preview sculpt. Recommend the
coarsest measured level where the radius is at least the mean edge
length. If all levels can be measured and none qualifies, say that a
new subdivision level may give a closer result. If the drop point cannot
be mapped to a level well enough to measure it, do not recommend that
level; disclose that the estimate is incomplete, or unavailable if none
can be measured. If no measured level qualifies but some cannot be
measured, say that there is no suitable measured level rather than
claiming that no suitable level exists. This is a resolution heuristic,
not a promise that the shape will match: local topology, curvature and
the target surface also matter.

The app does not switch or add levels automatically. Show the current
level and the recommendation before the drop; the user can cancel and
choose a level first. Warn that switching or adding a level bakes
existing living strokes on the piece. After placement, switching or
adding a level also bakes the part: the shape remains on the piece but
its living strokes, whole-part editing and recipe-based save back are
no longer available. Coarser levels may not visibly represent all of
the detail. If the active level is coarser than the heuristic, the drop
is still allowed with a warning.

### Mirroring

The plane passes through the piece's symmetry centre (`SYMC`) and is
perpendicular to the axis you chose. If the piece has no centre, the
drop uses the same estimate the brushes use and does not write `SYMC`.

The mirrored strokes are a second replay: the same recipes, with
samples and the frame reflected through that plane, symmetry still
off, in the same group, linked as above. Points are not snapped to a
nearest vertex the way Mirror copy snaps, because the recipes are run
again on the surface. A sample that does not land is counted, as in
the curvature rules.

Radial symmetry is not a drop choice. The axes are X, Y and Z.

**On the symmetry plane.** The footprint is the part's mapped samples
at the current scale, before this adjustment. Its width along the
mirror axis is the distance from its lowest sample to its highest
along that axis. The tolerance is 5% of that width. When the width
is shorter than the mean edge at the drop, the tolerance is that
mean edge, so a stamp still counts as on the plane. The drop is on
the plane when the anchor is within that tolerance of it.

Every such drop asks. The last answer is not reused. The usual
**Place one** / **Mirror pair** choice is replaced by a prompt at
the pointer:

```text
On the mirror plane, X
[ One centered copy ]
[ Mirror, halves match ]
[ Cancel ]
```

The axis is the one **Mirror pair** would have used. When none or
several of Mirror X/Y/Z are on, the prompt lists X, Y and Z, with X
selected. Esc or **Cancel** cancels the drop. The preview comes off,
the mesh is as it was before the drag, and nothing is recorded.

- **One centered copy** shifts the part along the mirror axis until
  the midpoint of the footprint lies on the plane, then places it
  once. Samples may lie on both sides. There is no second side and
  no link.
- **Mirror, halves match** shifts the part along the axis until it
  lies on one side and the sample closest to the plane lies on the
  plane. The side is the one that already holds more of the
  footprint, and the positive side when the two are equal. Samples
  the shift brings across the plane move with the part. That side is
  then mirrored through the plane as a linked pair. The two halves
  meet on the plane, and each is the reflection of the other.

### Undo

| Action | Undo step |
|---|---|
| Drop: Place one, Mirror pair, One centered copy, or Mirror, halves match | One step, "Place *name*" |
| Move, turn or scale on the surface | One step, "Move *name*" |
| Edit…, Edit path, fade, delete, Mirror, Unlink | One step each, as the Strokes window already records |
| Preview, Esc, Cancel on the on-plane prompt, a refused drop | None |

The place step snapshots every chunk the replay touches before it
changes them (`History::touch`), then commits the mesh, the new
strokes, the group and the placement record together. Undo removes
that group and restores the chunks.

### Saving in the project

The strokes and their recipes are the existing `STRK` and `STRI`
sections. Their meaning does not change.

The placement is a new skippable section, `PART`, written after `STRI`
for that piece. Older versions skip it. The surface still opens as it
looks, and a version that reads `STRI` can still Edit… each stroke. It
only loses "this group is one part", the whole-part move, and the
link. A `PART` whose version byte is newer than the reader knows is
skipped the same way; the strokes remain.

```text
PART
version:u8 = 1
count:u32
per placed part:
  id:u64
  name:text                      # len:u32, then UTF-8, as in a recipe
  group:u32                      # stroke group on this piece
  n:u32  stroke_id:u64 * n       # replay order
  anchor:f32*3  normal:f32*3  tangent:f32*3
  scale:f32                      # 1 = recorded size
  detail_size:f32                # the radius the file divided by
  content:u8*32                  # SHA-256 of the .oxydetail at drop, or 32 zero bytes
  flags:u8                       # bit 0: linked; bit 1: source side
  mirror_axis:u8                 # 0 none, 1 X, 2 Y, 3 Z
  partner:u64                    # the other placed part's id, else 0
```

A single copy has `mirror_axis` 0, `partner` 0, the source-side flag set
and the link flag clear. A pair is two records, each naming the other,
both linked, with the same axis; exactly the placement made from the bin
has the source-side flag. Unlink does not change these roles. If an
entire side is deleted, the survivor retains its role even though its
partner is gone. **Save back to bin** finds the library file by the SHA-256
(`hash.` in `library.txt`). The file name is a hint; the hash wins
after a rename. The hash identifies a local file. It is not a security
check. `library.txt` is not copied into the `.oxb`.

### Import, export, rename, delete, reorder, categories

**Import…** copies a `.oxydetail` into the library by the same atomic
write, sanitises the name, and queues a thumbnail if there is no
sidecar PNG. **Export…** copies that `.oxydetail` out. It does not
overwrite the source project, and it does not upload anything.

**Rename** changes the display name and the file name together (the
detail, the PNG and the `library.txt` keys). **Delete** asks "Delete
*name* from the Parts Bin? Parts already placed on a model stay there."
and removes the detail and the PNG. **Reorder** is a drag inside the
open slot's variation row; `order` in `library.txt` is rewritten.
**Category** is a short label on the part (Eyes, Ears, Arms). Empty
means no category. Parts that share a category are the variations
shown when any of them is opened. The strip itself is one slot per
part, in library order, not one slot per category.

### Thumbnails

The core crates do not render. `oxy-sculpt` and `oxy-doc` store and
replay recipes with no GPU. The picture is made in the app, through
`oxy-render`.

On save, the app asks the renderer for a square view aimed at the
anchor, cropped to the part's bounds, using the piece as it is then,
and writes a 128×128 PNG beside the detail by the atomic rename. After
the project is closed, that PNG is what the slot shows. Import of a
file with no PNG renders the part once onto a neutral sphere, in an
offscreen pass, so the slot is not blank; if that replay misses the
sphere, the pass uses a plane. A missing or failed image shows the
part's name in the slot. The thumbnail is regenerated on **Save back
to bin** and **Save as new part**, and is never required to place the
part.

The PNG is a cache. It is not a test oracle: a software renderer and a
real GPU will not match pixel for pixel.

### What cannot be a part

The same strokes that Edit… cannot edit, from phases 1 and 2 of RFC
0025, cannot be saved. The dialog uses a sentence of the same shape as
the Edit… text, and saves the strokes that do have recipes. If none
do, it saves nothing.

| Reason | What the dialog says |
|---|---|
| Made before recipes, or the recipe did not load | "This stroke was made before this version kept what was drawn, so it can't be a part. It can still be faded or deleted." |
| Dynamic detail was on | "This stroke was made with dynamic detail on, which changes the triangles under it, so it can't be a part. It can still be faded or deleted." |
| Curve Move, Deform, the gizmo | These are not in the stroke list. The window says they change the whole piece and are not strokes. |
| A mirror copy, or one side of a split | "This is a mirrored copy, or one side of a stroke that was split, so it can't be a part. Save the original stroke." |
| Radial symmetry | "Radial symmetry repeats this stroke around the model, so it can't be saved as one side." |
| Living strokes off | "Living strokes is off. Turn it on in Preferences > General to save or place a part." |

A mixed selection reports how many were left out and the reasons. The
strokes that were left out stay on the piece, unchanged.
The mirror side of a **placed part** is an exception to the ordinary
Mirror copy rule: it has its own recipes and can explicitly be saved as
a new part. It is not silently substituted for the source on save back.

## Performance

- Saving a part copies recipes. It adds nothing to the per-dab path.
- The only new per-frame work is the preview, and only while a drag or
  a Move on surface is active. It uses the Edit… budget: full replay
  at or under 32 ms estimated, coarser spacing above that, and a gap
  of at least 50 ms or 1.5 times the last preview.
- A commit over 32 ms estimated leaves the UI thread via
  `Editor::start_job`. One job covers the whole part, in stroke order.
- Opening the bin reads `library.txt` and the PNG files. Recipes are
  read when a drag starts.
- The subdivision recommendation is computed for the drop, not on the
  per-dab path. Measuring existing levels must not mutate the piece or
  switch its active level; expensive work runs off the UI thread.
- The thumbnail is one offscreen draw per save, not per frame.
- Placed part recipes count toward the separate, user-adjustable Parts
  memory limit. The library on disk does not.
- The implementation reports `dab_timing` for a multi-stroke replay.
  The bar to aim at: on a mesh of about a million triangles, a part of
  a few thousand dabs stays inside the preview rule above. The numbers
  come from that run.

## Phases

Each phase can ship on its own and has tests that fail if it is
wrong. Phases 1 and 2 are how RFC 0025 phases 4 and 5 land; they are
not a second implementation beside them.

1. **Save and place, and it stays editable.** The detail (anchor, one
   side, `.oxydetail`) and a place action from the Strokes window:
   the hit under the cursor, scale and twist handles, one undo step,
   a stroke group whose strokes keep recipes. Edit… works on them.
   Later strokes stay stored. The dialog explains anything that cannot
   be a part. The strip is not in this phase; the place action is the
   one the strip will call. Tests: a replay on a second mesh matches a
   direct replay of the mapped recipe; symmetry flags on the stored
   recipe do not double the stroke; undo restores the mesh and removes
   the group; Edit… changes the placed stroke and leaves a later
   stroke's stored change in place; a dynamic-detail stroke is refused
   with its sentence.

2. **The strip.** The data-folder library, thumbnails, slots, paging,
   category filter, drag onto the mesh, the Place one / Mirror pair
   choice, the on-plane rule, and the curvature, size, mask, dynamic
   detail, subdivision and Parts memory-limit messages. Import and
   export of `.oxydetail`. Tests: the library name cannot leave the folder;
   atomic replace leaves no `.tmp` behind; a drop records one undo
   step and a cancel records none; a mirrored drop has recipes on
   both sides and symmetry off; a drop on the plane asks, and Cancel
   leaves the mesh unchanged; One centered copy puts the footprint
   midpoint on the plane; Mirror, halves match meets on the plane; a
   drop while dynamic detail is on does not change the mesh and shows
   its reason; a drop that would exceed the Parts memory limit does
   not change the mesh, bake ordinary strokes, or use more than the
   cap, and explains how to raise the limit or free Parts memory. The
   subdivision recommendation tests cover scaled brush radii, a
   suitable existing level, no suitable level and an unmeasurable
   level; neither the estimate nor the drop silently switches or adds
   a level.

3. **After the drop.** Move on surface (move, turn, scale) as one
   re-sculpt and one undo step. The `PART` section round-trips, and a
   reader that skips `PART` still loads `STRK` and `STRI`. Linked
   **Edit…**, **Edit path**, and **Move on surface** propagate to the
   other side in the same undo step. Fading or deleting one stroke
   does not. Unlink lets the user disable linked edits and keeps both
   recipes.
   **Save back to bin** and **Save
   as new part** write only when asked, and an Edit… test asserts the
   library file is byte-for-byte unchanged. A later non-part stroke
   survives a move as stored. Edit path needs no extra work in this
   phase: it applies when RFC 0025 phase 3 ships. Tests place an
   asymmetric part on the negative side, edit its mirror, save and
   reopen, then save back and check the original handedness. They also
   cover unlink, source-side fade and deletion, deletion of the whole
   source side, and refusal to overwrite after baking. A level change
   bakes the sculpt into the piece and ends the part's editability; a
   bin save before the change remains usable on another piece.

4. **Housekeeping.** Rename, delete, reorder, categories, and the
   variations in the open slot. Tests: delete removes the library file
   and leaves a placed part's strokes in the project; reorder survives
   a restart of the index; a shared category appears as the variations.

5. **Along the surface.** When RFC 0025 phase 6 defines the geodesic
   walk, placement and Move on surface use it instead of the
   projection. The switch is the whole phase. A test on a bent strip
   checks that a band keeps its recorded length within the tolerance
   that phase 6 sets. Until then, the stretch warning from phase 2 is
   the behaviour.

## Tests

Headless, beside the checks above:

- `oxy-sculpt`: map a recipe from a plane onto a sphere and back;
  scale multiplies radii and offsets together; omitted samples are
  counted; a list replay returns one stored change per stroke, in
  order; reflecting a frame across X, Y and Z matches the samples.
- `oxy-doc`: `PART` round-trip; a file with `PART` renamed to an
  unknown section still loads the strokes; a newer `PART` version is
  skipped; `.oxydetail` round-trip once its layout is fixed in the
  RFC 0025 phase 5 work.
- `oxy-app`: the editor tests named in the phases, including refusal
  when a part would exceed the Parts memory limit, no mesh change on
  refusal, and the message explaining how to raise the limit or free
  Parts memory.
- The thumbnail has no golden image. When the strip exists, a
  headless screenshot (`--screenshot`) is taken of a bin with one
  part, for a person to look at. A unit test asserts that a part
  still loads and places when the PNG is absent.

`cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace` and `python3 scripts/check_corpus_ip.py` are
the usual bar. No crate outside `oxy-render` and `oxy-app` gains a
GPU dependency.

## Risks

- The preview can be too coarse on a large part, so the committed
  sculpt surprises you. The full replay on drop, and Esc on the job,
  are the mitigation. The `dab_timing` numbers will show if the 32 ms
  bar is too low.
- Resolution recommendations are estimates, not a way to recover fine
  detail on a coarse target. Switching levels also bakes living strokes;
  the warning must come before the user commits to the switch.
- Projection stretches a part that wraps. The warning is honest, and
  the geodesic walk is the real fix. Shipping the warning without the
  walk is a known limit.
- People will expect the bin to update when they edit the placed
  part. The confirm on **Save back to bin** is there so it never
  happens quietly. The risk is that the command is hard to find.
- Resolution recommendations are estimates, not a way to recover fine
  detail on a coarse target. Switching levels also bakes living strokes;
  the warning must come before the user commits to the switch.
- The two sides of a linked pair can sit at different strengths,
  while **Edit…** still changes both. That follows the decision that
  fade and delete are per side. A later edit updates the faint side's
  recipe and leaves its strength as it was.
- An old version skips `PART`. The model looks right, and per-stroke
  Edit… still works where `STRI` is read. Whole-part move and the
  link do not. That is acceptable, and it should be stated in the
  release notes of the version that adds `PART`.
- A drop that fights the brush: the drag has to start on a slot, so a
  stroke that starts on the mesh is still a stroke. Worth a test that
  a press on empty viewport does not pick up a part.
- The shelf docked at the bottom, and the Touch layout, can crowd the
  strip. The strip stays on the viewport, one row, and can be hidden.
- Thumbnail colours will differ between GPUs. The PNG is a cache, not
  a format invariant.
- Saving the positive side of a symmetric stroke may keep the half the
  user does not think of as the original. The rule is deterministic and
  stated in the save behavior.

## Alternatives considered

- **Inserted meshes, or surface instances ([RFC 0022](0022-surface-instances.md)).**
  Those copy triangles and follow the host. A part is strokes sculpted
  again, so it stays in the Strokes window and edits like the work
  around it. Inserted triangles were excluded by RFC 0025 and stay
  excluded.
- **A new file format.** `.oxydetail` already has the recipes, the
  anchor and the alphas. A second format would split the library.
- **Storing both sides in the file.** Rejected in RFC 0025. The drop
  choice rebuilds the other side from the one stored side.
- **Always following Mirror X/Y/Z, with no question at the drop.**
  That is the RFC 0025 rule. The sketch asks for a choice. The
  toggles remain the default axis.
- **A modal Accept that ends placement.** After Accept, the part would
  be ordinary strokes with no whole-part move. The requirement is that
  move, turn and scale still work later, so the placement record is
  kept and Accept is not the end.
- **Writing the bin on every edit.** The placed part and the library
  would stay identical, and a shared file would change under you. The
  explicit save commands avoid that.
- **Extending `STRK` with the link and the frame.** That would change
  the meaning of a section older versions already read. `PART` is a
  new section they skip.
- **A software thumbnail in `oxy-sculpt`.** It would keep the picture
  GPU-free everywhere, and it would be a second renderer to maintain.
  The render crate already draws the view.
- **An online library of parts.** Out of scope. No network beyond the
  two opt-in features the project already has.
- **The geodesic walk in the first phase.** It is the right mapping
  for a band around an arm, and it is already RFC 0025 phase 6. Doing
  it inside the first drop delays a shippable projection that matches
  the written detail design.

## Decisions

The owner decided these on 2026-11-18.

- **One library.** Show it as the Parts Bin strip, not a separate
  Details section in the Strokes window.
- **Mirror choice on drop.** Keep one side in the file; ask whether to
  place one or a mirrored pair. Mirror X/Y/Z supplies the default axis.
- **Commit on drop.** Record one undo step and keep move, turn and
  scale available afterward.
- **Recipes on both sides.** A mirrored part has linked recipes on both
  sides; ordinary Mirror copies remain unchanged.
- **Default scale follows the brush.** The part's reference radius,
  the first saved stroke's brush radius, is drawn at the current
  brush's world radius. Other strokes scale by the same factor.
  **Original size** remains a choice and is not the default.
  **Size: On the model** selects that world radius: on the model it
  is the stored scene-unit size and zoom does not change the part; on
  screen it is the screen size converted at the drop, so zooming in
  makes the part smaller. See "Scale, twist, and the live preview".
- **Fade and delete are per side.** On a linked pair, the link covers
  **Edit…**, **Edit path**, and move, turn and scale. The strength
  slider and Delete on a stroke row affect that side only. The group
  row still fades, solos and deletes both sides. The **Edit…** text
  says the change applies to the other side and that fade and delete
  do not.
- **Dynamic detail refuses the drop.** The status line reads "Dynamic
  detail is on, which changes the triangles, so this part can't be
  placed. Turn it off." The mesh is unchanged. This is the same rule
  as phases 1 and 2: those strokes are not editable, and a placed
  part has to stay editable.
- **On the symmetry plane, ask every time.** Within 5% of the part's
  footprint along the mirror axis (or one mean edge, when the
  footprint is narrower than that), the drop asks **One centered
  copy** or **Mirror, halves match**. The answer is not remembered.
  Esc or **Cancel** cancels the drop and records nothing.
- **One Parts Bin library.** There is one library, shown as the Parts
  Bin strip, with **Save as part…** and placed groups in the Strokes
  window; there is no separate Details section.
- **Choose placement at the drop.** The drop asks **Place one** or
  **Mirror pair**. The piece's Mirror X/Y/Z setting selects the default
  axis.
- **Commit on drop.** Dropping commits placement as one undo step.
  Move, turn and scale remain available afterwards.
- **Recipes on both mirrored sides.** A mirrored part stores recipes
  on both linked sides. Ordinary Mirror copies remain unchanged.
- **Default twist uses a stable world axis.** The initial twist
  direction is derived from a fixed world axis, not the camera.
- **Both save commands are available.** **Save back to bin** replaces
  the part only after confirmation, and **Save as new part** creates a
  separate library entry.
- **Keep the positive side for new parts.** When saving new parts from
  originally symmetric strokes, keep samples on the positive side of
  the mirror axis; if several axes were on, positive X is kept. Strokes
  with radial symmetry are refused. This does not change the source-side
  rule for saving a placed pair back.
- **Store parts in the data directory.** The Parts Bin folder sits
  beside brush presets under the app's data directory.
- **Same-category parts are variations.** Opening a part shows the
  other parts in its category as variations.
- **Defer geodesic mapping to RFC 0025 phase 6.** Until that phase
  exists, placement and Move on surface use projection.
- **Linked edit propagation is user-selectable.** Mirrored parts start
  linked, so edits propagate in either direction. The user can choose
  **Unlink** to disable propagation; undo restores the linked state.
- **Parts have a separate memory limit.** Placed part strokes use a
  dedicated Parts memory pool, separate from the ordinary
  living-strokes budget. Its limit is user-adjustable. When a new
  placement would exceed it, refuse the placement without changing
  the mesh and explain that the user can raise the limit in
  Preferences or remove placed parts to free space.
- **Parts are strokes, not meshes.** A part has no vertices, topology
  or subdivision levels of its own; its recipes are replayed on the
  target piece's active level.
- **Keep bin-entry handedness on save back.** Save back uses the
  placed pair's source side, regardless of which side was edited or
  where it lies relative to the mirror plane. If the source side is
  gone, do not silently replace it with its mirror.
- **Subdivision levels belong to the piece.** Recommend a suitable
  existing level before placement, but never switch or add levels
  automatically. A level change bakes the part's living strokes.

## Not included

- Parts that insert or cut triangles. RFC 0025 leaves those out.
- Strokes that survive remesh or a subdivision change. They could be
  replayed from recipes; RFC 0025 leaves that to a separate RFC. A
  baked part is baked.
- A shared online library, accounts, or any new network call.
- Radial symmetry as a placement option.
- New plugin or agent commands. A placed stroke is already a living
  stroke, so the existing stroke list and stroke commands see it.
  `oxybrush-cli mcp` has no living strokes and stays that way.
- Bundled starter parts. If any are added later, they are made for
  OxyBrush and are CC0, as RFC 0025 says for starter details.
- A new shortcut, and any change to the in-app shortcut list.
- "Replay later strokes" as part of a move. It stays the optional
  action RFC 0025 already defers.
