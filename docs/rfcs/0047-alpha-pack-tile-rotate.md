# RFC 0047 — Better alphas: pack, tile, roll, rotate

- **Status:** Phase 3 (scale pack) in progress — Scales rebuilt; Snake scales,
  Diamond scales, Scale plate added. Phase 1–2 and **Roll** still open
- **Builds on:** [RFC 0030 — Alpha library](0030-alpha-library.md),
  alphas in Phase 3.3 ([ROADMAP](../ROADMAP.md))
- **Related:** [RFC 0041 — Paint color jitter](0041-paint-color-jitter.md)
  (sampling-mode pattern), [RFC 0046 — Scale cover](0046-scale-cover.md)
  (geometry scales vs painted scale texture), stencils (Phase 3.3, still open)
- **Name in UI:** keep **Alpha** / stroke settings. New controls: **Tile**,
  **Roll**, **Roll distance**, **Rotate jitter** (exact labels TBD). Do
  **not** use other products’ proprietary feature names in the UI
  ([AGENTS.md](../../AGENTS.md) trademarks). Do **not** ship third-party
  alphas whose license is unclear.

## Problem

Artists rely on alphas for scales, pores, fabric, armor trim, ropes, chains
and painted detail. Four gaps show up together in testing:

1. **Built-in quality.** The procedural **Scales** alpha was a soft
   overlapping half-disc pattern. People reach for downloaded packs; many
   have unclear licenses. OxyBrush may only ship **CC0 / CC-BY /
   MIT-Apache-BSD / zlib-compatible** assets ([AGENTS.md](../../AGENTS.md)).
2. **Seamless inside a dab.** Alphas sample with **0 outside** the square
   (`Alpha::sample`). There is no wrap/tile mode inside one stamp.
3. **Continuous along a stroke (“roll”).** Discrete dabs re-stamp the same
   UV each time. A paint-roller stroke — the alpha advances along the path
   so a tileable rope, chain or barbed-wire pattern continues unbroken —
   is missing. Follow-stroke only rotates the stamp; it does not scroll UV
   by path length. (Public demos of this idea are often called “Alpha
   Roll”; OxyBrush ships it as **Roll**.)
4. **Random rotation.** **Spray** already scatters dabs and varies alpha
   rotation with spray amount. There is no independent **rotate jitter**
   for a normal freehand stroke.

Scale *geometry* (overlapping plates that catch light) is **Scale cover**
(RFC 0046), not this RFC. This RFC is about **height/paint stamps**.

## Decision

| Piece | What | Why |
|---|---|---|
| **A. Alpha pack** | Curated extra stamps (procedural and/or embedded CC0 PNGs) | Better defaults without users hunting licenses |
| **B. Tile** | Optional wrap sampling + density (repeats per brush radius) | Seamless pattern *inside* one dab |
| **C. Roll** | Path-length UV along the stroke + Roll distance | Continuous tileable detail (rope, chain, trim) |
| **D. Rotate jitter** | Amount + Per Stroke / Per Dab, independent of Spray | Breaks up identical stamp orientation |

### A. Alpha pack

**Prefer original procedural alphas** in `oxy-sculpt`. Add a small set aimed
at what testers reach for first (scales first, then pores, fabric, rivets,
bark as needed). Embedded PNGs only when procedural cannot match, with
explicit CC0/compatible credits.

**Capture from view** and **Load image…** stay the path for personal stamps.
Non-square loaded alphas already pad into a square; Roll wants the *pattern*
to tile along one axis — document that artists author tileable strips
(or use Capture) for best results.

**Started:** overlapping U **Scales**, **Snake scales**, **Diamond scales**,
**Scale plate**.

### B. Tile (seamless inside a dab)

Alpha option **Tile** (off by default):

- Off: UV outside `[-1, 1]` samples **0** (current).
- On: wrap UV; **Repeats** = periods across the brush diameter. Brush
  falloff still softens the dab edge; the *pattern* tiles inside.

Use for fabric, chainmail, brick, scale grids authored to tile.

### C. Roll (paint-roller along the stroke)

**Roll** (off by default) changes how freehand (and similar) strokes sample
the alpha:

- Across the stroke (brush width): UV stays local to the dab, like today
  (`offset · bitangent / radius`).
- Along the stroke: UV advances with **arc length** from stroke start,
  scaled by brush radius and **Roll distance** (default 1 = one alpha
  period per ~diameter of travel; higher stretches, lower compresses).
  Sample with **wrap** on that axis.
- Orientation: same as **Follow stroke direction** (tangent = path
  direction in the surface plane). Turning Roll on implies follow-stroke
  while active, or requires it — pick one in implementation and document it.
- Spacing still emits dabs; each dab’s along-UV is keyed by path distance
  at the dab centre so overlapping dabs share a continuous pattern instead
  of re-stamping UV zero.
- Stamp / Line / Curve stroke modes: Roll applies where a continuous path
  length exists; Stamp (single dab) ignores Roll.

**Roll distance** is a float ≥ a small epsilon (e.g. 0.1–10), stored on the
brush preset and stroke recipe.

This is the feature that makes custom tileable strip alphas (rope, chain,
barbed wire, rivet runs, scale rows along a limb) paint cleanly. Dab-local
**Tile** (B) does not replace it.

Implementation sketch (design only; write from scratch in `oxy-sculpt`):

1. Accumulate path length on the stroke (already have spacing carry /
   samples).
2. In alpha weight: `u = dist / (radius * roll_distance)` wrapped;
   `v = offset · bitangent / radius` (clamp or wrap per Tile).
3. Keep radial brush falloff on the dab so the strip edges soft-blend.

### D. Rotate jitter

Mirror Paint color jitter:

- Amount in degrees; **Per Stroke** vs **Per Dab**; seeded; independent of
  Spray; symmetry copies share the sample.
- Added on top of manual rotation + follow-stroke / Roll orientation.

## Compatibility

- Defaults (Tile off, Roll off, jitter 0) → existing strokes unchanged.
- New fields on presets and stroke recipes (versioned).
- New built-ins additive in `BuiltinAlpha::ALL`.
- No new `.oxb` section beyond recipe/preset encoding.

## Phases

| Phase | Scope |
|---|---|
| **1** | Rotate jitter (Per Stroke / Per Dab) + guide/tooltip |
| **2** | Dab **Tile** wrap + Repeats |
| **3** | Expanded built-in set (procedural first). **Started:** scale variants |
| **4** | **Roll** + Roll distance; headless probe that a tileable strip alpha
  paints a continuous pattern along a bent freehand path; guide tip;
  optional starter procedural strip (e.g. rope / rivets) if useful |

Phase 4 is the high-impact gap vs dab Tile alone. Prefer shipping Roll
before more pack fillers once Phase 3’s scale slice is solid.

## Acceptance criteria

- Defaults match today’s alpha behaviour.
- Per Dab jitter changes orientation without Spray.
- Tile + Repeats wraps inside a dab; Tile off still zeros outside.
- With Roll on and a tileable strip, a curved freehand stroke shows one
  continuous pattern along the path; changing Roll distance stretches or
  compresses it; Roll off restores discrete stamps.
- Living-stroke replay keeps Roll / distance / seed.
- Docs: ROADMAP 3.3, CHANGELOG, guide; CLEAN_ROOM entry for public Roll
  sources (no proprietary names in the UI).

## Non-goals

- Replacing Scale cover (geometry plates).
- Copying another product’s UI, icons, alpha library or names.
- Shipping unverified Gumroad / Discord alpha packs.
- Full screen-space stencils, UV paint, or world-locked projection
  (separate work).
- AI-generated alphas without a clear CC0 dedication and human review.
