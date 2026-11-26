# RFC 0054 — Split view (multiple viewports)

**Status:** Accepted (Phase 1)
**Issue:** user request (multi-viewport)

## Summary

Let a document show **two viewports** at once — side by side or stacked —
each with its own camera. The scene, selection, brush and undo history stay
shared. One pane is **active**: orbit, pan, zoom and sculpt go there.

## Motivation

Comparing front and side, or a close-up next to an overview, is a common
sculpting habit. OxyBrush today has one camera and one canvas rectangle.

## Phase 1

| Control | Behavior |
|---|---|
| **View > Split view** | Off · Left / right · Top / bottom |
| Divider | Drag to resize (ratio remembered for the session) |
| Active pane | Click a pane to activate; a thin accent rim marks it |
| Cameras | Independent orbit / pan / zoom / ortho / frame / view snaps |
| Shared | Scene, pieces, brush, mask, living strokes, materials |

Session preference (`split_view` in settings). Not saved in `.oxb` yet.

### Rendering

`ViewportRenderer::render` gains `clear: bool`. The first pane clears the
surface; the second loads and draws only into its scissor/viewport rect.
Depth clears per pane. One GPU mesh upload serves both.

### Input

`Editor::canvas` is the **active** pane rect (physical pixels), as today.
`canvas_full` is the union used to lay out panes. On press, the pane under
the pointer becomes active (cameras swap so `Document::camera` stays the
active one; existing stroke/orbit code is unchanged).

## Out of scope (later)

- Three or four panes, custom grids
- Per-pane shading / wireframe / X-ray
- Linked cameras (same target, locked orbit)
- Persist cameras and layout in `.oxb`
- Painting into a non-active pane without activating it

## Acceptance

- Split left/right and top/bottom; Off restores one pane
- Each pane keeps its own view across toggle Off → On
- Sculpt and orbit only move the active pane’s camera
- Headless `--demo` / screenshot still use a single canvas
- `cargo test -p oxy-app` pane layout helpers; clippy clean
