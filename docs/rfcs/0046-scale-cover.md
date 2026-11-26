# RFC 0046 — Scale cover (“Dino mode”)

- **Status:** Proposed (design for review; not scheduled for v4.0.0)
- **Builds on:** [RFC 0022 — Surface instances](0022-surface-instances.md)
- **Related:** [RFC 0045 — Surface Noise](0045-surface-noise.md), built-in
  **Scales** alpha (`BuiltinAlpha::Scales`)
- **Name in UI:** **Scale cover** (primary). Optional short label / tip:
  **Dino mode** (Discord nickname). Do **not** use proprietary product
  names for the feature (AGENTS.md trademarks).

## Problem

Creature and hard-surface artists want overlapping **scale plates** (and
related types: armor tiles, studs, ridges) covering a sculpt — denser on
the back, softer on the belly, different shapes per region — without
hand-placing thousands of copies.

OxyBrush already has three related tools that are not enough alone:

| Tool | What it gives | Gap for “dino” |
|---|---|---|
| **Surface instances** (RFC 0022) | Live copies of a source piece on faces / points / groups / scatter; Make real | No scale-oriented presets, multi-type covers, or plate library |
| **Scales alpha** + Draw | Painted height bumps that look like scales | Soft texture only; not real plates that catch light as geometry |
| **Surface Noise** (RFC 0045) | Whole-piece procedural displace | Base grit, not discrete overlapping plates |

Discord nicknamed the ask **“dino mode.”** There is no dedicated GitHub
issue yet; this RFC is the design record.

## Decision

Add **Scale cover** as a **productized workflow on top of surface
instances**, not a parallel remesh or a new mesh format.

1. The host keeps one or more **instancers** (existing `INST` / `INS2`
   data). Scale cover is a UI + preset layer that configures those
   instancers for scale work.
2. Ship a small set of **original, CC0-compatible scale source meshes**
   (generated in OxyBrush or authored for the repo — never copied from
   other products’ assets).
3. Artists pick a **type**, **placement** (mask / face group / even
   scatter), **size / spacing**, and **orientation**, then refine the
   source with **Edit source**. **Make real** bakes when ready.
4. Optional underlay: run **Surface Noise** (or the Scales alpha) on the
   host first so plates sit on slightly rough skin.

### Naming

| Context | Name |
|---|---|
| Menu / window / docs | **Scale cover** |
| Tip / welcome one-liner (optional) | “Dino mode” as a nickname only |
| Code / sections | `scale_cover` UI module; still `Instancer` / `INST` underneath |
| Forbidden | Proprietary feature names from other sculpt apps |

## Design

### Scale types (Phase 1 library)

Each type is a **source piece recipe**: a low-poly plate (or spike) with a
clear base facing −Y (or a documented local frame), so instances can align
“base to surface.”

| Id | Intent |
|---|---|
| `teardrop` | Classic overlapping reptile scale |
| `diamond` | Pointed / fish-like |
| `round` | Soft pebble / armadillo |
| `plate` | Flat armor tile |
| `ridge` | Long raised strip (along flow) |
| `spike` | Outward thorn (height dominant) |

Artists can also pick **any piece** as the source (custom scale sculpt).
Built-ins are starters only.

### Placement (reuse RFC 0022)

| Mode | Scale-cover default |
|---|---|
| **Even scatter** | Primary: Poisson-disk on visible / masked / group faces (Cline et al. 2009, already cited for instances) |
| **Mask** | Cover only where the mask is above a threshold |
| **Face group** | Different types per body region (belly vs back) |
| **Per face** | Optional for hard-surface tiling, not the dino default |

**Density** is controlled by minimum spacing (scene units or relative to
piece extent) plus absolute / relative **size** and **lift** along the
normal. Random **spin / scale / offset** use a seed (same as today’s
instancer vary).

### Orientation

Reuse instancer orient knobs, with scale-friendly defaults:

- **Normal** = plate “up” (out of the surface).
- **Forward** default = **longest edge** or a future **flow** direction
  so rows of scales share a grain on limbs; **world direction** when the
  artist wants no flips; **random** for chaotic hide.
- Symmetry follows the host (RFC 0022).

### Multi-type covers

One host may have **several** scale-cover instancers (already allowed:
several instancers per host):

- Instancer A: `teardrop` on face group “Back”, dense.
- Instancer B: `round` on group “Belly”, sparse and flatter.
- Instancer C: `spike` on masked crest.

Phase 1 ships this as **manual** multiple covers. Phase 2 may add a
**type map** (face group → type, or vertex-color id → type) that builds
those instancers in one Apply.

### Editing and bake

Unchanged instance workflow:

- **Edit source** — sculpt one plate; every copy updates.
- **Edit placement** — sculpt / re-mask the host; anchors rebuild.
- Hand exclusions (when Phase 2 of RFC 0022 lands fully) — paint away
  bad copies.
- **Make real** — ordinary mesh + face groups; triangle estimate first.

### Relationship to Surface Noise and alphas

- **Underlay (optional):** Surface Noise or Scales alpha on the host
  *before* covering, for pores between plates.
- **Not a substitute:** Scale cover does not replace Surface Noise;
  plates are instances, not displacement.
- **After bake:** further Surface Noise / brushes on the realized mesh
  are normal sculpting.

### Clean room

- Idea source: Discord request (“dino mode”) for covering a sculpt with
  scale-like detail of different types.
- Algorithms: surface instances plan only — Poisson-disk scatter and
  (later) shell-map fitting already logged in
  [CLEAN_ROOM.md](../CLEAN_ROOM.md) for RFC 0022.
- Do **not** open proprietary scale / nanomesh UIs, icons, presets, or
  source. Do not copy other products’ scale meshes or layout.

## Non-goals

- Physics / collision between plates.
- Per-copy unique sculpt without Make real (RFC 0022).
- Automatic retopo of the host under the cover.
- Shipping photogrammetry animal skins or third-party asset packs.
- Replacing Surface instances UI for non-scale uses (rivets, bolts, hair
  cards stay on the Instances panel).

## Phases

| Phase | Ships |
|---|---|
| **1** | Scale cover window/panel; built-in plate library; configure one or more instancers (type, mask/group/scatter, size, spacing, seed, orient); Edit source / Make real via existing instance ops; docs + CLEAN_ROOM |
| **2** | Multi-type from face groups in one Apply; flow orientation; tighter scatter-on-mask/group; hand exclusions when RFC 0022 exposes them |
| **3** | Fitted plates (depends on RFC 0022 fitted copies); optional type from vertex color; underlay Noise preset button |

Phase 1 is blocked only on finishing the remaining RFC 0022 placement
polish the cover UI needs (scatter on group/mask if not already enough).
Fitted “chainmail bend” waits on RFC 0022 phase 3.

## UI sketch (not a layout copy)

- Tools > **Scale cover…** (and command palette).
- Type gallery (thumbnails of built-in plates + “Use active piece”).
- Placement: Scatter / Mask / Face group.
- Sliders: Size, Spacing, Lift, Variation, Seed.
- Orient: Normal + Forward choice.
- Actions: Apply / update cover, Edit source, Make real, Remove cover.
- Tip may mention the Discord nickname once; menus stay **Scale cover**.

Exact widget arrangement is original to OxyBrush.

## Verification (when implemented)

- Unit / editor tests: applying a cover creates an instancer with the
  expected source and placement; Make real triangle count matches
  estimate within tolerance; undo removes the cover.
- Masked / grouped placement leaves uncovered regions empty.
- Seed stability: same seed → same anchors.
- AGENTS checks; headless screenshot of a covered sphere for the PR.
- No proprietary assets in the library (license note next to sources).

## Open questions for review

1. Should built-in plates live as bundled `.oxb` / mesh blobs, or be
   generated procedurally at first run?
2. Is **Dino mode** only a tip nickname, or also a window subtitle?
3. One window vs a mode inside the existing Instances panel?
4. Priority vs finishing RFC 0022 fitted copies first?
