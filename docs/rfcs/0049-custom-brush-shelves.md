# RFC 0049 — Custom brush shelves and preset editor (Phase 1)

- **Status:** Phase 1 implemented
- **Builds on:** Brush presets (`.oxbrush` / `oxyb1:` codes), shelf
  reorder in preferences
- **ROADMAP:** “Brush presets and editor” / “custom shelves of favorites”

## Problem

Presets already save, apply, share as codes and files. The shelf still
shows every brush kind, and finding a named preset means opening
**Presets…**. Artists want a short **favorites** strip and quicker preset
management without a full redesign.

## Decision

### Phase 1 (this change)

1. **Favorite brushes** — star/unstar from the shelf context menu; a
   **Favorites** strip above the full shelf (order = favorite list).
   Persisted as `favorite_brushes` in `settings.txt`.
2. **Favorite presets** — star in **Presets…**; starred names sort first.
   Persisted as repeated `favorite_preset = …` lines.
3. **Preset editor-lite** — **Rename** and **Duplicate** in the preset
   context menu (still the popup, not a separate window).
4. **Command palette / Space** — `Preset: <name>` entries that apply a
   library preset.

The full brush list remains; favorites do not hide kinds (avoids changing
the “always all brushes in `brush_order`” invariant).

### Phase 2 (later)

- Dedicated brush editor window (edit a preset by name without sculpting).
- Multiple named shelves / workflow tabs.
- Preset tiles on the shelf; hide unused brush kinds.
- MCP tools for the preset library.

## Naming

| Context | Name |
|---|---|
| Shelf strip | **Favorites** |
| Docs / ROADMAP | Custom shelves / brush editor (Phase 1) |
| Settings keys | `favorite_brushes`, `favorite_preset` |
