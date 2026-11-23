# RFC 0039 — Per-brush Accumulate

**Status:** Proposed

## Summary

Add **Accumulate** next to the dab controls for Draw, Inflate, Clay, and
Clay Strips. Default to on to preserve existing brush behavior.

## Motivation

Closely spaced dabs currently build on the preceding dabs. A user drawing
a line or applying a textured alpha needs control over whether overlapping
dabs raise or carve the same point again.

## Design

With Accumulate off, a point receives up to the strongest dab's
displacement within the current stroke. A stronger dab adds the remaining
difference. The next stroke starts with a fresh limit. Gravity and
auto-smooth remain separate effects. Layer already limits buildup and
continues to use its own height rule.

Store the switch per brush, including when other brush settings are
shared. Save and restore it with presets and remembered settings, and
record it in living-stroke recipes. Old presets and recipes retain
accumulation. Keep the existing recipe byte version for strokes with
accumulation enabled.

## Validation

Check repeated dabs, higher pressure, inverted strokes, stroke boundaries,
settings round trips, and legacy presets and recipes.
