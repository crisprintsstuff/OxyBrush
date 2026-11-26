# RFC 0047 — Better alphas: pack, tile, rotate

- **Status:** Proposed
- **Builds on:** [RFC 0030 — Alpha library](0030-alpha-library.md),
  alphas in Phase 3.3 ([ROADMAP](../ROADMAP.md))
- **Related:** [RFC 0041 — Paint color jitter](0041-paint-color-jitter.md)
  (sampling-mode pattern), [RFC 0046 — Scale cover](0046-scale-cover.md)
  (geometry scales vs painted scale texture), stencils (Phase 3.3, still open)
- **Name in UI:** keep **Alpha** section; new controls **Tile**, **Rotate
  jitter** (exact labels TBD in implementation). Do **not** ship third-party
  alphas whose license is unclear.

## Problem

Artists rely on alphas for scales, pores, fabric, armor trim and painted
detail. Three gaps show up together in testing:

1. **Built-in quality.** The procedural **Scales** alpha
   (`BuiltinAlpha::Scales`) is a soft overlapping half-disc pattern. It is
   fine as a starter, but it does not read as clean stylized scales, so
   people reach for downloaded packs. Random free packs often have unclear
   or incompatible licenses; OxyBrush may only ship **CC0 / CC-BY /
   MIT-Apache-BSD / zlib-compatible** assets ([AGENTS.md](../../AGENTS.md)).
2. **Seamless repeating.** Alphas sample with **0 outside** the square
   (`Alpha::sample`). There is no wrap/tile mode, so painting a seamless
   fabric or chainmail sheet means fighting stamp edges, or hoping Spray
   hides seams. Screen-space **stencils** (Phase 3.3) are still open and
   solve a related but different job.
3. **Random rotation.** **Spray** already scatters dabs and varies alpha
   rotation with spray amount. There is no independent **rotate jitter**
   for a normal freehand stroke (fixed spacing, no scatter) — the same
   pattern as Paint’s color jitter (per stroke / per dab) without forcing
   Spray on.

Scale *geometry* (overlapping plates that catch light) is **Scale cover**
(RFC 0046), not this RFC. This RFC is about **height/paint stamps**.

## Decision

Ship three incremental improvements under the existing Alpha / brush
settings surface:

| Piece | What | Why |
|---|---|---|
| **A. Alpha pack** | Curated extra stamps (procedural and/or embedded CC0 PNGs) | Better defaults without users hunting licenses |
| **B. Tile** | Optional wrap sampling + density (repeats per brush radius) | Seamless repeating textures while sculpting or painting |
| **C. Rotate jitter** | Amount + Per Stroke / Per Dab, independent of Spray | Breaks up identical stamp orientation along a stroke |

### A. Alpha pack

**Prefer original procedural alphas** in `oxy-sculpt` (same path as today’s
seven built-ins): no binary bloat, no license table per PNG, easy to tweak.
Add a small set aimed at what testers reach for first, for example:

- Cleaner **stylized scales** (sharper plate edges, clearer overlap grain)
- **Snake / fish** tighter row scales
- **Pores / orange peel**
- **Fabric wrinkle** (soft folds)
- **Armor rivet / panel** hard-surface stamp
- **Bark / cracks** refinement (or keep Cracks and add one more)

Only add **embedded PNG** alphas when a procedural version cannot match
the look. Any PNG must be:

- Clearly **CC0** (or another AGENTS-compatible license) with a credit line
  in Help > About and next to the file (same pattern as human base meshes /
  matcaps).
- Checked in as grayscale 16-bit where practical, sized for stamp use
  (e.g. 512–2048), not a dump of an entire commercial pack.
- Reviewed for trademarked silhouettes or copies of proprietary brush
  alphas — **original or properly licensed only**.

Candidate external CC0 sources for evaluation (not yet selected; each
asset needs its own license check before commit): ambientCG sculpt
wrinkles, other verified CC0 heightmaps. Prefer generating OxyBrush’s own
plates over bulk-importing packs.

**Capture from view** and **Load image…** (library, RFC 0030) stay the
path for personal / project-specific stamps. Do not expand the shipping
binary with dozens of rarely used maps.

### B. Tile (seamless repeat)

Add an Alpha option **Tile** (off by default):

- When off: current behavior — UV outside `[-1, 1]` samples **0** (hard
  stamp edge after radial falloff of built-ins).
- When on: sample with **wrap** (fractional UV), and a **Repeats**
  control (how many periods of the alpha fit across the brush diameter).
  Brush falloff still softens the dab edge so strokes do not show a hard
  square; the *pattern* tiles inside the dab.

Use Tile for fabric weave, chainmail, brick, scale grids and other
patterns authored to tile. Non-tileable images will show seams — that is
expected; document it in the guide.

**Out of scope for this RFC:** full screen-space stencils, UV-channel
painting, and projecting a locked world/object texture across the whole
mesh. Those remain Phase 3.3 / later; Tile is dab-local wrap only.

### C. Rotate jitter

Mirror Paint color jitter’s UX shape:

- **Rotate jitter** amount in degrees (0 = off; e.g. 0–180).
- **Per Stroke** (one random offset for the whole stroke) vs **Per Dab**
  (new offset each dab).
- Seeded from the stroke so living-stroke replay and presets stay
  deterministic; Reseed later if testers ask.
- Independent of **Spray**: Spray may still add its own scatter and
  rotation when spray > 0; jitter applies even with spray at 0.
- Symmetry copies of a dab share the same rotation sample (same rule as
  color jitter).

**Follow stroke direction** stays as today; jitter is added on top of the
base rotation (manual rotation + follow-stroke).

## Compatibility

- Default Tile off and Rotate jitter 0 → existing strokes unchanged.
- New fields on brush presets and stroke recipes (versioned); old recipes
  decode as Tile off / jitter 0.
- New built-in alphas are additive in `BuiltinAlpha::ALL`; names must not
  collide with library entries.
- No `.oxb` section change beyond recipe/preset encoding already used for
  brush settings.

## Phases

| Phase | Scope |
|---|---|
| **1** | Rotate jitter (Per Stroke / Per Dab) + guide/tooltip; tests on dab rotation determinism |
| **2** | Tile wrap + Repeats; headless probe that a tileable procedural alpha paints without edge drop-off inside the dab |
| **3** | Expanded built-in set (procedural first); optional small CC0 PNG pack with credits if needed |

## Acceptance criteria

- With jitter and Tile at defaults, goldens / existing alpha tests match.
- Per Dab jitter changes orientation along a freehand stroke without Spray.
- Tile + Repeats on a procedural checker (or similar) shows wrap inside the
  dab; Tile off still zeros outside the square.
- Every new shipping alpha has an explicit license note (procedural =
  original CC0; PNG = credited CC0/compatible).
- Docs: ROADMAP 3.3, CHANGELOG, guide topic for Alpha Tile / Rotate jitter.

## Non-goals

- Replacing Scale cover (geometry plates).
- Shipping unverified Gumroad / Discord alpha packs.
- Stencils, UV paint, or world-locked projection (separate work).
- AI-generated alphas without a clear CC0 dedication and human review.
