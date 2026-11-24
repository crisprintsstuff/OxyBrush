# RFC 0042 — Reference image planes

**Status:** Draft

## Summary

Add a **References** panel for loading local images into the sculpting
viewport. Start with axis-aligned image planes for matching a sculpt to
front, side, and top references.

## Motivation

Artists can compare proportions and silhouettes while sculpting when the
reference stays aligned with the model as they pan and zoom. A project
should reopen with the same reference images and placements on another
machine.

## Design

- **View > References** opens a list of the project's images. Add a PNG
  or JPEG, assign a Front, Side, or Top orientation, and adjust its
  position, scale, and opacity. Preserve the image's aspect ratio.
  Each image can be renamed, hidden, or removed. Edits are undoable.
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

## Acceptance criteria

- Align front and side images to the same model. Verify pan, zoom,
  orbit, view snapping, opacity, and the overlay option.
- Place multiple images, change their settings, and undo and redo
  transformations, visibility changes, and removal.
- Save and reopen the project after moving the image files. Check a
  second project tab and confirm each project shows its own references.
- Sculpt and pick a retopology surface with an overlay enabled. Export
  the mesh and verify the reference geometry does not appear in the
  export.
- Try oversized, corrupt, and transparent images. Verify bounded
  resource use, legible errors, and correct alpha blending.
- Measure viewport frame time with references hidden and visible.

## Alternatives

A floating image board is useful for mood and anatomy references that
need no alignment. It could share the image library in a later pass.
Retain it as a separate interaction because its images stay in screen
space while the artist moves the camera.
