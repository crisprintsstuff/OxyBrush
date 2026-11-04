# Clean-room feature work

OxyBrush is MIT OR Apache-2.0. Some sculpting tools that inspire its feature
list are under copyleft licenses (for example GPL-3.0), whose code must not
be copied, translated or paraphrased into OxyBrush (see AGENTS.md, "Licenses"
and "Clean-room behavior").

When a feature is inspired by another product, the rule is:

1. **Only public, user-facing material is consulted:** feature lists, user
   guides, screenshots, videos. **Never source code** (not even to "see how
   it's done"), and never decompiled or leaked material.
2. The feature is **designed and written from scratch** for OxyBrush, from
   its own architecture, published papers (cited in comments) and general
   knowledge.
3. File formats, names and assets are OxyBrush's own. Trademarked feature
   names of other products are not used (AGENTS.md, "Trademarks").
4. The inspiration is recorded below, so reviewers can check the boundary.

## Log

| Date | Source consulted | What was taken | Features |
|---|---|---|---|
| 2026-11-04 | SculptForge (GPL-3.0-or-later) README feature list only; its source was never opened, and the local clone used to read the README was deleted | The idea of each feature (one sentence each) | Brush presets with shareable one-line codes and preset files; command palette and type-to-pick brush picker; recent files and an offline guide; posing gizmo; vertex color painting; glTF/PLY export, LOD chains, splitting pieces, a headless CLI |

Not taken, by design: paid-membership prompts, community/online galleries,
and cloud AI services. A local MCP server ([RFC 0003](rfcs/0003-mcp.md)) can
let a program on this machine drive OxyBrush; it does not call out to a
model or any other host. The only other network code is the opt-in updater
([RFC 0002](rfcs/0002-updates.md)).
