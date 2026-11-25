# RFC 0037 — Geometry-only 3MF export

**Status:** Implemented ([#308](https://github.com/crisprintsstuff/OxyBrush/issues/308))

## Summary

Export visible pieces as separate mesh objects in a 3MF Core package.
Write sizes in millimeters using the configured scene-unit conversion.

## Motivation

A slicer should receive a sculpt at the expected physical size and let the
artist select individual pieces. An STL file cannot declare its units or
carry independently named mesh objects.

## Design

- Add 3MF alongside the existing export formats. Use the same piece
  visibility rules and full-detail geometry as the existing scene export.
- Write one mesh object and one build item per exported piece. Keep
  relative piece placement. Convert OxyBrush's Y-up coordinates to
  Z-up with a rotation that preserves triangle winding.
- Use the existing **Scene unit size** print setting and declare
  millimeters in the model. Show the scale in the export UI so the
  artist can check the intended size before writing.
- Use a compatible pure-Rust ZIP writer and the existing atomic export
  path. Keep import, materials, slicer settings, and extensions for later
  milestones.

## Validation

Check that a two-piece fixture opens in a slicer as two mesh objects at
the expected size and relative positions. Test Unicode piece names,
hidden pieces, subdivision levels, and cancellation or write failure.
