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
| 2026-11-05 | Published academic papers only; OpenVDB source code (MPL-2.0) was never opened, consulted, or adapted | Algorithmic and mathematical principles from published literature: Museth (2013) ACM TOG 32(3) for hierarchical sparse B-tree structure; Ju et al. (2002) ACM SIGGRAPH for dual contouring; Bærentzen & Aanæs (2005) IEEE TVCG 11(3) for angle-weighted pseudonormal SDF signing; Ericson (2005) §5.1.5 for triangle distance | Hierarchical sparse VDB grid (`crates/oxy-vdb`), mesh-to-volume level set conversion, dual contouring polygonization, CSG booleans, level set offsets and uniform hollowing shells |
| 2026-11-07 | A user asked for editable curve strokes after seeing them in Nomad Sculpt 2.9 (proprietary). Its manual could not be read from the development environment. Later the user shared a 15-second phone recording of a public tutorial video showing the feature, which was viewed (frames only; no audio, documentation or code) | The behaviour seen in the clip: a drawn stroke stays adjustable, with its result shown live on the model, and can be reshaped and extended by dragging. No UI layout, icons, names or assets were taken. A second, 15-second clip of the same video (also frames only) showed the editing session: per-point size, an optional confirmation, undo while editing, closed curves; those behaviours were reimplemented with OxyBrush's own controls (a window with sliders, not a toolbar) | Curve stroke mode ([RFC 0004](rfcs/0004-curve-stroke.md)): interaction, fades, preview and apply designed from scratch for OxyBrush |
| 2026-11-07 | A user's one-sentence recollection, in [#82](https://github.com/crisprintsstuff/OxyBrush/issues/82), of a feature another app had planned and never shipped (curves used to move parts of a mesh). Nothing of that app was consulted | The idea only | Curve Move ([RFC 0005](rfcs/0005-curve-move.md)): built on OxyBrush's own curve editor, with the published wire-deformer (Singh & Fiume 1998) and rotation-minimizing-frame (Wang et al. 2008) methods, cited in `crates/oxy-sculpt/src/wire.rs` |
| 2026-11-08 | A user's request in [#81](https://github.com/crisprintsstuff/OxyBrush/issues/81) for a dynamic-topology mode like the ones in other sculpting apps. No documentation, video, code or UI of those apps was consulted | The idea only (a mode where the brush adds and removes triangles as it sculpts; detail tied to brush size is a common, generic design) | Dynamic detail ([RFC 0006](rfcs/0006-dynamic-detail.md)): split/collapse rules from Botsch & Kobbelt (SGP 2004), cited in `crates/oxy-mesh/src/dyntopo.rs` |

Not taken, by design: paid-membership prompts, community/online galleries,
and cloud AI services. A local MCP server ([RFC 0003](rfcs/0003-mcp.md)) can
let a program on this machine drive OxyBrush; it does not call out to a
model or any other host. The only other network code is the opt-in updater
([RFC 0002](rfcs/0002-updates.md)).
