# RFC 0041 — Paint color jitter

**Status:** Implemented

## Summary

Add independent hue, saturation, and value variation to the Paint brush.
Let artists sample a color once when the stroke begins or once per dab.

## Motivation

Small variations help break up uniform color in hand-painted textures.
One color per stroke gives consistent marks with variation across strokes.
Per-dab variation gives texture within a single stroke. See [issue #476](https://github.com/crisprintsstuff/OxyBrush/issues/476).

## Design

Put **Color Jitter** in the Paint brush settings. Hue, Saturation, and
Value each default to zero. Hue sets the maximum shift in either direction
in degrees (0–180°). Saturation and Value set maximum shifts in percentage
points (0–100). Sample each offset independently and uniformly within its
range. **Per Stroke** is the default sampling mode. **Per Dab** draws a new
sample for each emitted dab, regardless of pointer event frequency.

Convert the selected sRGB paint color to HSV before applying the offsets.
Wrap hue and clamp saturation and value. Use zero hue for an achromatic
input, so hue jitter alone leaves grays neutral and saturation jitter can
tint them. Value jitter can lift black or darken white. Convert the sampled
color back to sRGB before the existing vertex blend. Zero amounts and
the paint eraser use the existing paths.

Give every new stroke a fresh seed. Derive color samples from that seed
and, in Per Dab mode, the index of the emitted dab. Use a separate random
sequence from spray and alpha rotation. Compute the sampled color before
the symmetry loop, so all copies of a dab share the same color. Reopening
or editing a stroke keeps its seed. Changes to spacing may change where
samples land because they change where dabs are emitted. A Reseed control
can be reconsidered after artist feedback.

### Storage and replay

Persist the amounts and sampling mode with the Paint brush preset. Record
the settings and seed in the editable-stroke recipe so a stroke can be
replayed on its own. The current recipe stores the base sRGB paint color.
Extend its versioned encoding and decode older recipes with the amounts
set to zero. Keep existing project sections and existing paint values.

### Sampling experiment

A headless model blended 28 dabs over white using a smoothstep radial
falloff and strength 0.6. It used the same offsets at both spacings, a
base sRGB color of (0.75, 0.22, 0.18), and maximum offsets of 25° hue
and 18 percentage points each for saturation and value. Each offset was
sampled with Python's `Random(0xC014 + 3 * dab + channel).uniform(-1, 1)`
for this experiment only.

| Distance between dab centers | Per Stroke | Per Dab |
| --- | ---: | ---: |
| 0.2 × brush radius | 0.0001 | 0.0367 |
| 1.2 × brush radius | 0.0000 | 0.1317 |

Values are the mean sRGB distance between consecutive interior dab centers
after blending, excluding the first and last three dabs. Overlap softened
the per-dab variation by about 72% at tight spacing. On middle gray, hue
alone left all 28 samples neutral. With saturation jitter enabled, 9 of
28 samples showed a tint. This supports starting with the two sampling
modes. The experiment models color blending along a line. Check the feel
on a sculpt mesh before deciding whether a distance-based mode is needed.

## Acceptance criteria

- Existing Paint brushes give the same output with jitter at zero.
- Each amount can vary independently and always produces valid colors.
- Per-stroke mode uses one sample throughout the stroke. Per-dab mode
  varies along the stroke with matching colors across symmetry copies.
- Repeat a stroke with the same seed and inputs and compare vertex colors.
- Verify undo, redo, old and new preset round-trips, and editable-stroke replay.
- Compare densely spaced and widely spaced strokes on a mesh. Check gray,
  black, white, and saturated colors with each slider enabled on its own.
- Measure dab cost with jitter off and with jitter enabled.

## Alternatives

A fixed color palette would make discrete color choices easy to control,
but would require a separate palette editor. Start with variation around
the chosen color.
