# ZBrush format — reverse-engineering methodology

> How we figure out the `.ZTL` / `.ZPR` formats. Disciplined, repeatable,
> lesson-driven. Adapted from the SWFormat methodology
> (<https://github.com/KenM76/swformat>, `docs/REVERSE_ENGINEERING.md`).
> Every rule here is subject to [`LEGAL.md`](LEGAL.md): **file bytes only,
> no decryption, no circumvention, no analysis of the ZBrush program.**

---

## 1. The diff-pair method

The fundamental primitive:

1. **Build a minimal-variation pair of files** — two files that differ in
   exactly one knowable way, saved by ZBrush from your own work:
   - `sphere_a.ztl` vs `sphere_b.ztl` — same tool, saved twice with no
     changes. **Reveals: save-time non-determinism** (timestamps, GUIDs).
   - `sphere_div1.ztl` vs `sphere_div2.ztl` — one extra subdivision level.
     **Reveals: where vertex/face counts and multires data live.**
   - `sphere_1sub.ztl` vs `sphere_2sub.ztl` — a second SubTool.
     **Reveals: the SubTool list structure.**
   - `sphere_pg1.ztl` vs `sphere_pg2.ztl` — one polygroup changed.
     **Reveals: per-face group storage.**
   - `sphere_mask0.ztl` vs `sphere_mask1.ztl` — mask fully on vs off.
     **Reveals: per-vertex mask storage.**
   - `sphere_moved.ztl` — one vertex moved by a known amount.
     **Reveals: position encoding (float width, endianness, quantisation).**
2. **Run the diff tool** at every layer:
   ```bash
   cargo run -p oxy-ztl --bin ztl-diff -- a.ztl b.ztl
   ```
   It reports raw-byte change regions (Layer 0), changes inside embedded
   compressed streams (Layer 1/2), and size/offset shifts.
3. **Form a hypothesis** in the hypothesis log (§3).
4. **Falsify it**: build further pairs that must produce the same change
   if the hypothesis is right; or edit the bytes at the suspected location
   in a *copy* and confirm ZBrush opens it with the predicted change
   (Layer 3, §5).
5. **Write the lesson** once it survives the falsification checks (§5).
6. **Encode the lesson** as a parser + writer with tests. The test is the
   standing falsification: if it breaks, the lesson is wrong.

## 2. The corpus is the lifeblood

- Synthetic fixtures: trivially simple files you created from scratch
  (e.g. a default sphere, one SubTool) and dedicate as CC0. Planned:
  ZScript macros under `tools/corpus_gen/zbrush/` that regenerate them
  deterministically in each ZBrush version, so the committed corpus stays
  IP-clean and is regeneratable.
- Real files: referenced by absolute path in
  `test/corpus/zbrush/corpus.config.json` with `license_status: owner`; the
  harness runs against those that exist locally and skips the rest.
- Each fixture eventually gets a **golden manifest** (expected parse output
  as JSON) under `test/corpus/zbrush/golden/`.

## 3. Hypothesis log

Each active topic gets `docs/formats/zbrush/research/<topic>/log.md`
(samples and scratch decoders stay local; `research/**/samples/` is
gitignored):

```markdown
# <topic>

## 2026-11-02 — initial hypothesis
<what we think and why; which diff pair suggested it>

## 2026-11-03 — falsified
<the experiment that broke it; exact bytes/offsets>

## 2026-11-04 — confirmed
<the experiments it survived>

## 2026-11-04 — graduated
Lesson: lessons/lesson_20261104_<slug>.md · Handler: crates/oxy-ztl/src/<file>.rs
```

**Falsified hypotheses stay in the log** so nobody re-investigates them.

## 4. The tail-bytes discipline

When parsing any structure, read every byte you intend to write back.
There is no such thing as an "obviously unused" byte in a proprietary
format. Either:

- the bytes have meaning → decode them;
- the bytes are unused → they should be zero; assert it and pass through;
- the bytes are non-deterministic → list them in the stable mask and pass
  through.

In `oxy-ztl` every decoded binary structure keeps a `tail_bytes: Vec<u8>`
field with everything not yet understood, written back verbatim.
`tail_bytes` only ever shrinks; it is the visible measure of progress.

At Layer 1 the same rule is the **no-orphan-bytes invariant**: concatenating
every record of a `ZtlDocument` reproduces the input exactly
(`ZtlDocument::reconstruct`), enforced by tests and by `ztl-echo`.

## 5. Falsification before a finding becomes a lesson

Every finding is checked three ways before it is encoded:

1. **Cross-version** — files from several ZBrush versions (e.g. 2021,
   2022, 2023, 2024, 2025). Version-specific findings are flagged as such.
2. **Cross-type** — `.ZTL` and `.ZPR` (where the structure exists);
   single- and multi-SubTool; with and without subdivision levels, layers,
   polypaint.
3. **Edge cases** — empty SubTool, very large meshes, non-ASCII SubTool
   names, maximum subdivision levels.

**Layer 3 (ZBrush-equivalence):** a write-side finding is only accepted
once ZBrush opens a file produced by `oxy-ztl` and shows the predicted
result. Layer-3 checks need a ZBrush license and are run manually by
maintainers; CI runs Layers 0–2 only.

## 6. Prior art

No license-compatible open-source `.ZTL` reader is known to the project at
the time of writing. If one is found, it may be used as an algorithm
reference **only if** its license is MIT/BSD/Apache-2.0 compatible, with:

1. a one-paragraph description of the algorithm in
   `research/<source>_notes/<file>.md`;
2. an implementation written from that description;
3. cross-checked output on the same input files.

This is a license-compatible port, **not** a clean-room process, and the
project does not claim otherwise. GPL/AGPL code must not be ported.

## 7. When to stop

Some data is not worth decoding, or must not be:

- **Encrypted / protection-related regions** → detect, report as
  unsupported, stop (LEGAL §1). Never attempt to break them.
- **Thumbnails, UI state, caches** that ZBrush regenerates → opaque
  pass-through forever.
- **Third-party plug-in data** → never decode; pass through verbatim.

Choosing to leave a region opaque is itself a documented finding (write a
lesson saying why).

## 8. Failure-mode catalog

Grows as findings accumulate. Initial entries:

| Symptom | Likely cause |
|---|---|
| `ztl-dump` finds no compressed streams and entropy ≈ 8 bits/byte throughout | Whole-file compression with an unknown codec, or encryption. Do **not** proceed past detection until it is established which; if encryption, stop (LEGAL §1). |
| Twin saves differ in length | Embedded timestamps/strings or recompression non-determinism; build the stable mask (roadmap M0.5) before judging any other diff. |
| Echo (read→write) not byte-identical | Layer-1 bug: a record dropped or re-encoded. P0. |

## 9. Lesson template

`docs/formats/zbrush/lessons/lesson_<YYYYMMDD>_<slug>.md`:

```yaml
---
date: 2026-MM-DD
category: format-spec | quirk | workflow | crash
severity: high | medium | low
subject: oxy-ztl
structures_touched: [header, subtool-list, ...]
zbrush_versions_tested: [2023, 2024, 2025]
file_types_tested: [ztl, zpr]
related_lessons: []
---
```

Body: **Context** · **What we found** (byte ranges, hex dumps, version
stamps) · **How we verified** (diff pairs, falsification attempts) ·
**Implementation** (file path) · **Limits** · **References**.

The lesson is the single source of truth. If code and lesson disagree, the
lesson wins until it is updated with a dated footer.
