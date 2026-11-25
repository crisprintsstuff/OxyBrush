# RFC 0042 — Reference image planes

**Status:** Phases 1–3 implemented (viewport X-ray, reference planes, and live image alignment).

## Summary

Add a **References** panel for loading local images into the sculpting
viewport. Start with axis-aligned image planes for matching a sculpt to
front, back, right, left, top, and bottom references. Add viewport X-ray so
artists can see those images through the sculpt.

## Motivation

Artists can compare proportions and silhouettes while sculpting when the
reference stays aligned with the model as they pan and zoom. A project
should reopen with the same reference images and placements on another
machine.

## Design

- **View > References** opens a list of the project's images. Add a PNG
  or JPEG, assign a Front, Back, Right, Left, Top, or Bottom orientation,
  and adjust its position, scale, and opacity. Preserve the image's aspect
  ratio. Right keeps the earlier Side wire index so older projects load.
  Each image can be renamed, hidden, or removed. Placement edits preview
  in the viewport as the controls change. Apply records a single undoable
  edit. Revert or close the panel to discard the current project’s preview.
- Show an image by default when the orthographic view matches its
  orientation. Offer a per-image option to show the plane in perspective
  and other views for artists who orbit while working. Place the plane
  behind the model by default. Support an overlay option at reduced
  opacity for aligning landmarks.
- Keep image planes separate from mesh pieces. Sculpting, selection and
  retopology raycasts pass through them, and geometry exports contain only
  the model. Adjust references from their panel. This leaves the
  primary viewport gesture available for sculpting.
- Embed bounded, compressed image data and the plane settings in an
  optional project section. Older readers can skip the section and load
  the sculpt. Decode imports off the UI thread, with limits on decoded
  dimensions and total image memory. Upload textures on import, restore,
  or replacement and reuse them on subsequent frames.

### Image alignment

- Rotate an image in 90° steps about its center and flip it across either
  axis of the plane. A quarter-turn preserves the image scale and aspect
  ratio. Flips use the plane axes after rotation.
- Preview these changes with the placement controls. Apply records the
  complete placement as one scene undo entry.
- Save the transforms in unused placement flags in the existing `REFI`
  section. Older readers still display the embedded image with its
  original orientation. Existing projects start with zero rotation and
  both flips off.

### Landmark alignment

From an image's placement controls, start **Align to landmarks**. Pick
two points on the image, then the corresponding two points on the sculpt
in the matching orthographic view. Project all four points onto the
reference plane so placement depth stays fixed. Translate, rotate, and
uniformly scale the image to match the points, then preview the result
before Apply. Undo the last pick while collecting points. After four picks,
select any numbered image or sculpt landmark to replace it independently.
Use fit returns the result to the placement controls for Apply. Cancel
returns to the prior preview. Viewport clicks are reserved for alignment
while the picker or fit review is open.

The picker temporarily draws the selected image over the sculpt for the
first two clicks and hides reference images for the next two. A fine
rotation is persisted in an optional `RROT` section. Earlier readers
still load the embedded image with its placement and quarter-turns.

### Viewport X-ray

Expose **X-ray** directly in the viewport toolbar and View menu. Right-click
the toolbar button for **Mesh opacity** and scope. The same settings are
available in References and Shading. Start with X-ray off and 35% mesh
opacity. Expose three scopes:

- **All visible pieces** fades the active sculpt as well as the surrounding
  pieces, so a reference behind the sculpt is visible while drawing or
  sculpting.
- **Active piece** fades the active mesh so geometry behind it stays opaque.
- **Inactive pieces** keeps the active piece opaque and fades other
  visible pieces. This also covers [issue #454](https://github.com/crisprintsstuff/OxyBrush/issues/454).

X-ray affects display. The active tool keeps its normal raycast, selection,
mask, symmetry, and visibility behavior. The mesh opacity control is
independent of each image's opacity. Hiding a piece still removes it from
the view. The default background order is the floor and background,
then behind-model references, then meshes. A reference marked Overlay
is composited above meshes, with tool cursors and guides on top.

Build the shared viewport X-ray pass first and validate it on ordinary
meshes before wiring in the reference image pass. Keep opaque pieces in
the existing shading path. For the initial X-ray pass, depth-test against
opaque pieces and resolve only the nearest translucent surface at each
pixel into a separate color layer. Composite that layer at the selected
opacity. This keeps overlapping and closed meshes from accumulating
opacity on each backface or internal surface. Test AO, cavity, shadows,
wireframe, and instances to verify that ghosted geometry does not
reintroduce an opaque silhouette through an auxiliary pass. The design
does not require a through-selection mode or full layered transparency.

## Acceptance criteria

- Align front and side images to the same model. Verify pan, zoom,
  orbit, view snapping, opacity, and the overlay option.
- With a reference behind an opaque sculpt, toggle X-ray and verify the
  image shows through at a useful opacity. Check opaque active pieces in
  Inactive pieces scope, depth against opaque foreground geometry, and
  that overlapping ghosted pieces do not accumulate opacity.
- Paint the same view with X-ray on and off and verify the same piece is
  hit. Repeat with hidden pieces, retopology snapping, and a reference
  overlay.
- Place multiple images, change their settings, and undo and redo
  transformations, visibility changes, and removal.
- Save and reopen the project after moving the image files. Check a
  second project tab and confirm each project shows its own references.
- Sculpt and pick a retopology surface with an overlay enabled. Export
  the mesh and verify the reference geometry does not appear in the
  export.
- Try oversized, corrupt, and transparent images. Verify bounded
  resource use, legible errors, and correct alpha blending.
- Measure viewport frame time with X-ray off and on and with references
  hidden and visible.

## Alternatives

A floating image board is useful for mood and anatomy references that
need no alignment. It could share the image library in a later pass.
Retain it as a separate interaction because its images stay in screen
space while the artist moves the camera.
