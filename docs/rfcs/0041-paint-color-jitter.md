# RFC 0041 — Paint color jitter

**Status:** Draft

## Summary

Add independent hue, saturation, and value variation to the Paint brush.
Let artists sample a color once when the stroke begins or once per dab.

## Motivation

Small variations help break up uniform color in hand-painted textures.
One color per stroke gives consistent marks with variation across strokes.
Per-dab variation gives texture within a single stroke. See [issue #476](https://github.com/crisprintsstuff/OxyBrush/issues/476).

## Design

Put Color Jitter in the Paint brush settings. Store three zero-default
amounts and a sampling mode with the paint brush preset. Apply jitter to
the selected paint color before blending it into vertices. Wrap hue and
clamp saturation and value. When all amounts are zero, use the existing
paint path.

Derive the sample from a stroke seed and, in per-dab mode, the dab index.
Reuse the same sample across symmetry copies of a dab. Keep the seed with
editable strokes so replay uses the same sample sequence for the same
stroke inputs. Review preset and editable-stroke serialization before
adding the settings, and preserve existing preset defaults on load.

## Acceptance criteria

- Existing Paint brushes give the same output with jitter at zero.
- Each amount can vary independently and always produces valid colors.
- Per-stroke mode uses one sample throughout the stroke. Per-dab mode
  varies along the stroke with matching colors across symmetry copies.
- Repeat a stroke with the same seed and inputs and compare vertex colors.
- Verify undo, redo, preset round-trips, and editable-stroke replay.
- Measure dab cost with jitter off and with jitter enabled.

## Alternatives

A fixed color palette would make discrete color choices easy to control,
but would require a separate palette editor. Start with variation around
the chosen color.
