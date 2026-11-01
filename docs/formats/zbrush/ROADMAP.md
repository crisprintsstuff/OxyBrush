# ZBrush format — roadmap

> Honest milestones, structured like SWFormat's. Each is independently
> useful. Effort is part-time person-weeks; feasibility is rated on the
> worst-case interpretation. **Nothing is known yet about the internal
> structure of `.ZTL`/`.ZPR`**: every estimate past M0.5 depends on what
> M0 finds (in particular whether data is plainly compressed or encrypted;
> encrypted data is out of scope — see LEGAL §1).

---

## Milestone 0 — Bootstrap (container probe + tooling)

**Pass criteria:** `oxy-ztl` reads any file into a `ZtlDocument` whose
records reconstruct the input byte-for-byte (no orphan bytes); `ztl-dump`,
`ztl-diff` and `ztl-echo` work; the corpus rules are enforced in CI; docs
satisfy the "reconstruct from docs alone" bar.

**Effort:** 1–2 days. **Status:** ✓ tooling done (this commit). ⏳ needs
the first real files run through `ztl-dump` by someone with ZBrush.

**Deliverables:**
- ✓ `docs/formats/zbrush/` — README, LEGAL, REVERSE_ENGINEERING, ROADMAP,
  FORMAT_GUIDE (empty findings), lessons/, research/
- ✓ `crates/oxy-ztl` — Layer 0/1 model (`Gap`, `ZlibStream` records),
  probe (zlib stream discovery, per-block entropy, string extraction,
  encryption heuristic), lazy echo writer
- ✓ CLIs: `ztl-dump`, `ztl-diff`, `ztl-echo`
- ✓ `test/corpus/zbrush/corpus.config.json` template + harness test that
  runs against local owner files and skips missing ones
- ✓ CI guard against committed ZBrush files
- ⏳ First `ztl-dump` reports from ZBrush 2023/2024/2025 files (owner files,
  results summarised in a hypothesis log — not the files themselves)

## Milestone 0.5 — Twin-save baseline (BLOCKS M1)

Save the same tool twice with no edits, several times, in each available
ZBrush version; measure where and how much the twins differ.

**Pass criteria:** ≥10 twin pairs analysed; non-deterministic regions
listed in a stable mask; hypothesis log written.
**Effort:** 1–2 days. **Feasibility:** HIGH (purely diagnostic).

## Milestone 1 — Container map

Identify the top-level structure: header/magic, version stamp, how the
file is divided (sections, tables, compressed blocks), and where the
SubTool list starts.

**Pass criteria:** Layer-1 records named for every byte range in ≥3 files
from ≥2 ZBrush versions; `tail_bytes` documented; echo still byte-exact.
**Effort:** 2–8 weeks. **Feasibility:** MEDIUM if the container uses a
standard codec; **stop condition** if regions are encrypted (LEGAL §1).

## Milestone 2 — Read SubTool list and counts (read-only)

Names, visibility, vertex/face counts per SubTool and per subdivision level.

**Pass criteria:** matches what ZBrush displays for every corpus file.
**Effort:** 2–6 weeks. **Feasibility:** MEDIUM.

## Milestone 3 — Read base-level geometry → OxyBrush pieces

Vertex positions and polygon indices of the lowest subdivision level (and
quads triangulated for OxyBrush), polygroups, mask. First user-visible
feature: **File → Import ZBrush tool (read-only, experimental)**.

**Pass criteria:** imported geometry matches an OBJ exported by ZBrush from
the same tool (vertex-for-vertex within float tolerance) on the corpus.
**Effort:** 4–12 weeks. **Feasibility:** MEDIUM-LOW (depends on M1).

## Milestone 4 — Highest subdivision level, polypaint, layers

**Effort:** months. **Feasibility:** LOW–MEDIUM. Multires reconstruction
may need ZBrush's subdivision rules to be matched exactly.

## Milestone 5 — Write / round-trip (CONDITIONAL)

Only if M1–M3 are solid **and** the legal review allows it. Writes always
"save as a copy", never overwrite. Every write path needs a Layer-3 check
(ZBrush reopens it) on every supported ZBrush version.

**Effort:** unbounded. **Feasibility:** SPECULATIVE.

## Fallback

If the format proves to be encrypted, or the legal review is negative, the
work stops at M0 and OxyBrush keeps interchange via open formats (OBJ, STL,
PLY, glTF, later USD).

## Effort summary

| Milestone | Effort (part-time weeks) | Feasibility |
|---|---|---|
| M0 bootstrap | < 1 | ✓ done (tooling) |
| M0.5 twin-save baseline | < 1 | HIGH |
| M1 container map | 2–8 | MEDIUM (stop if encrypted) |
| M2 SubTool list | 2–6 | MEDIUM |
| M3 base geometry import | 4–12 | MEDIUM-LOW |
| M4 subdivision/polypaint/layers | 12+ | LOW–MEDIUM |
| M5 write | unbounded | SPECULATIVE |
