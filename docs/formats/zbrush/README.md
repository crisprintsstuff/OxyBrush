# ZBrush file interoperability (`oxy-ztl`)

> **Goal.** Let artists open their own ZBrush work (`.ZTL` tools, later
> `.ZPR` projects) in OxyBrush, without any closed-source library.
>
> **Status (2026-11).** Milestone 0 tooling only. Nothing about the
> internal format has been decoded yet; no ZBrush file has been analysed by
> the project so far. **There is no import feature in the app.**
>
> **Honesty disclosure.** This is open-ended reverse-engineering research,
> following the SWFormat project's process
> (<https://github.com/KenM76/swformat>). Useful results depend on what the
> files contain; if the relevant data is encrypted, the work stops by rule
> (see [`LEGAL.md`](LEGAL.md)).

## Read in this order

1. [`LEGAL.md`](LEGAL.md) — what is and isn't allowed (file bytes only,
   no decryption, no circumvention, no ZBrush program analysis, corpus IP
   rules). **Required before contributing.**
2. [`REVERSE_ENGINEERING.md`](REVERSE_ENGINEERING.md) — the diff-pair
   method, hypothesis logs, tail-bytes discipline, falsification, lessons.
3. [`ROADMAP.md`](ROADMAP.md) — milestones, pass criteria, honest estimates.
4. [`FORMAT_GUIDE.md`](FORMAT_GUIDE.md) — what is known about the format
   (graduated lessons only).

## Layer model

```
Layer 4  API        ZtlTool → OxyBrush pieces (M3+)
Layer 3  Structures SubTool list, mesh arrays, polygroups… (M1+), each with tail_bytes
Layer 2  Streams    decompressed payloads of embedded streams
Layer 1  Records    Gap | ZlibStream — every byte owned by exactly one record
Layer 0  Bytes      the file
```

**Invariants:** no orphan bytes (records reconstruct the file exactly);
lazy round-trip (unmodified records are re-emitted verbatim, never
re-compressed); tail bytes (anything not understood is preserved verbatim).

## Tools

```bash
cargo run -p oxy-ztl --bin ztl-dump -- my_tool.ztl          # structure, entropy, strings, streams
cargo run -p oxy-ztl --bin ztl-dump -- my_tool.ztl --extract out/   # write decompressed streams
cargo run -p oxy-ztl --bin ztl-diff -- a.ztl b.ztl           # diff pair at every layer
cargo run -p oxy-ztl --bin ztl-echo -- my_tool.ztl           # read→write must be byte-identical
OXY_ZTL_CORPUS=~/my.corpus.json cargo test -p oxy-ztl --test corpus -- --nocapture   # your own files
```

Run them only on files you have the right to analyse. Share **findings**
(offsets, hex snippets of your own synthetic files, hypotheses), never
the files.

## Contributing rules (in addition to the main CONTRIBUTING.md)

1. **Documentation first** — update the hypothesis log / lesson / format
   guide before or with the code.
2. **Diff-pair backed** — every format claim cites the diff-pair experiment
   that established it and the falsification checks it survived.
3. **Tests before merge** — Layer-1 (records), Layer-2 (parse→serialize
   round-trip), and, for anything write-related, a documented Layer-3
   ZBrush-reopen check.
4. **Tail-bytes invariant** — every byte read is writeable.
5. **Lesson capture** — every non-trivial finding gets a lesson.
6. **Dual-audience comments** — prose explaining *why* for humans, plus
   explicit byte-offset tables, `INVARIANT:`/`PRECONDITION:`/`RETURNS:`
   labels and cross-references to lessons for LLM readers.
7. **Commit messages** use the area prefix `ztl:` and reference the lesson,
   hypothesis log and milestone.

## Trademarks

ZBrush is a registered trademark of Maxon Computer GmbH. OxyBrush and
`oxy-ztl` are independent, unofficial projects, not affiliated with,
authorized by, endorsed by, or sponsored by Maxon. References are
nominative, solely to identify the file format.
