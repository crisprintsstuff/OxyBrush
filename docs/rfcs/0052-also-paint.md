# RFC 0052 — Also paint (sculpt + vertex color)

- **Status:** Phase 1 implemented
- **Builds on:** Paint brush, Color Jitter (#476), living-stroke recipes
- **Name in UI:** **Also paint**. Do **not** use proprietary “RGB mode”
  product names (AGENTS.md trademarks).

## Problem

Artists coming from other sculpt apps expect to raise clay and lay vertex
color in one stroke. OxyBrush’s **Paint** brush only colors; sculpt brushes
only move. Switching brushes for every pass is slow for polypaint-style work.

## Decision

Add a per-brush **Also paint** checkbox on dab sculpt brushes that support
it. When on, each dab moves the surface as usual **and** blends the current
Paint color onto the same vertices (same falloff / strength / alpha as the
sculpt). Alt still inverts the sculpt (Raise/Carve); erase color with the
Paint brush.

### Naming

| Context | Name |
|---|---|
| Brush settings | **Also paint** |
| Code / presets / recipes | `also_paint` |
| Forbidden | Proprietary RGB / polypaint mode names as UI labels |

## Design

### Phase 1 (this change)

- `BrushSettings::also_paint` + `BrushKind::supports_also_paint`
- Applied in `apply_kernel` after the deform pass
- UI checkbox + color / Color Jitter when on
- Per-brush look / presets (`also_paint = true`)
- Stroke recipes version **9** (`also_paint:u8` after flags)
- Undo snapshots color when Also paint is on; viewport shows vertex color

### Out of scope for Phase 1

- Grab / Kelvin / Scene Project / cutters
- Separate paint strength from sculpt strength
- Alt erase while Also paint is on

## Testing

- `also_paint_colors_while_draw_moves_the_surface` in `oxy-sculpt`
- Recipe round-trip for `also_paint`
