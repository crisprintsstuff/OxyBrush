# M0 — container probe

## 2026-11-01 — opened
Tooling (`ztl-dump`, `ztl-diff`, `ztl-echo`) is ready. No ZBrush file has
been analysed yet. First step for anyone with ZBrush and their own files:

1. Save a default sphere as `sphere_a.ztl`, then save again with no changes
   as `sphere_b.ztl` (same file name, copy out between saves — saving to
   different names can embed the name and create spurious diffs).
2. `ztl-dump sphere_a.ztl` and record here: file size, first 32 bytes (hex),
   number and offsets of zlib streams found, entropy profile summary,
   encryption heuristic verdict, ZBrush version.
3. `ztl-diff sphere_a.ztl sphere_b.ztl` and record the changed regions.

Questions to answer before M1:
- Is the file a sequence of standard (zlib/deflate) streams, a single
  compressed blob, or something else?
- Does any region look encrypted (high entropy, no decodable codec)? If
  so, stop here per LEGAL §1 and record the finding.
