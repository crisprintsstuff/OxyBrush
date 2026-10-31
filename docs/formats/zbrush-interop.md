# ZBrush file interoperability: policy and plan

> **Status: not started. Blocked on legal review.** Please do not submit format analysis or code for this area until this document says work is open.

The long-term goal is to let artists bring existing ZBrush work (`.ZTL` tools, then `.ZPR` projects) into OxyBrush, and eventually to save it back.

## Why this needs its own process

- `.ZTL` and `.ZPR` are proprietary, undocumented formats that change between ZBrush versions.
- Reverse-engineering *for interoperability* is protected in many jurisdictions, for example EU Directive 2009/24/EC Art. 6 and US interoperability case law and DMCA §1201(f). End-user license agreements may still restrict people who have accepted them.
- This document is a project policy, not legal advice.

## Rules

1. **Legal review first.** Before any work begins, the project gets advice from an open-source legal resource (for example Software Freedom Conservancy or a law-school clinic).
2. **Analyze only file output.** Contributors may study only `.ZTL`/`.ZPR` files they created themselves. Decompiling, disassembling, debugging or tracing the ZBrush program is **forbidden**.
3. **Two teams, kept apart.**
   - The *analysis team* writes a public specification (`docs/formats/ztl.md`).
   - The *implementation team* writes Rust code **only** from that specification.
   - Contributors declare their team in their PR, and nobody serves on both for the same format area.
4. **Clean test data.** The test corpus contains only CC0 files made by contributors. Never commit or redistribute Maxon-supplied tools, brushes, materials, or files containing them.
5. **Isolation.** All code lives in an `oxy-io-zbrush` crate behind a Cargo feature, so it can be removed or moved to its own repository without affecting the core.
6. **Hardening.** The parser must be panic-free on any input, fuzzed with `cargo-fuzz`, enforce allocation limits, and contain no `unsafe`.
7. **Naming.** Use "ZBrush" only to describe compatibility, always with this attribution: *ZBrush is a trademark of Maxon. OxyBrush is not affiliated with or endorsed by Maxon.*

## Plan

| Step | Timing | Scope |
|---|---|---|
| 0. Use existing exports | Now / 1.0 | Users export OBJ (or other open formats their ZBrush version supports) and import that. Add a "Coming from ZBrush" guide that maps concepts: SubTools → pieces, polygroups → face groups. |
| 1. Legal review and spec | Alongside 1.x | A legal opinion, a draft spec, and a corpus covering several ZBrush versions. |
| 2. Read-only import | 2.x | `.ZTL`: base mesh, SubTools as pieces, polygroups, masks, vertex color, then multires levels. `.ZPR` comes later. Anything unsupported is listed in a clear report and never silently dropped. |
| 3. Write / round-trip | Conditional | Only if import is stable and legal review allows it. Save as a copy by default, and run round-trip tests against real ZBrush versions on every release. |

**Fallback:** if the formats turn out to be encrypted, or the legal answer is negative, OxyBrush keeps the export-based path from step 0 and pushes for open formats (USD, glTF) in sculpting workflows.
