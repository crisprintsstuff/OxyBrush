# RFC 0030: Imported alpha library

Status: Proposed for issue #425

## Summary

**Load image…** adds a PNG brush alpha to the user's OxyBrush alpha library.
It appears in the current tab, other open tabs, and future sessions. The
import selects the alpha in the tab that requested it.

## Why

Artists reuse the same stamps across projects. Loading a PNG currently
adds it to one project, so using it elsewhere requires finding the image
and importing it again.

## Design

- Store the alpha as a 16-bit grayscale PNG in the `alphas` folder next
  to the user's `matcaps` and `parts` libraries. Write atomically and add
  a numeric suffix when a name is taken. Keep the source file intact.
- Leave built-ins and viewport captures as they are. Images imported
  into a project and library alphas referenced by saved strokes are
  embedded in the existing `ALPH` section. Other library entries are
  available in the picker without increasing the project file size.
- If saving to the library fails, keep the loaded alpha in the importing
  project and explain the failure in the status bar.

## Compatibility

There is no change to the project format. Existing projects keep their
embedded alphas when opened on a machine with a different library.
