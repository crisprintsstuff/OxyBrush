# RFC 0031: Per-brush pressure curves

Status: Proposed for issue #426

## Summary

The Stroke panel shows strength and size pressure curves for the active
brush. With **Settings per brush** enabled, each curve can use the global
default or have its own two-point shape.

## Why

A light touch can build up clay gradually while a crease brush needs more
pressure before it bites. Artists should be able to tune those responses
independently, including how the same pen pressure changes brush size.

## Design

- **Customize** starts from the corresponding default in **Preferences >
  Pen & tablet**. Drag either point to shape the curve. **Use global**
  removes that override. Response, Lightest touch and Full press remain
  tablet calibration shared by all brushes.
- The existing pressure switches still decide whether pressure affects
  strength and size. Per-brush curve shapes follow the **Settings per
  brush** preference and are remembered with the other brush settings.
- Brush presets and share codes carry the optional overrides. An older
  preset or settings file with no override uses the current global curve.
- Strokes capture the curves in use when they start, using the existing
  recipe data for replay. There is no project format change.
