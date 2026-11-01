# ZBrush file interoperability — legal posture

> Plain-English statement of the project's intended legal basis for the
> `oxy-ztl` work. **Not legal advice.** Adapted from the posture of the
> SWFormat project (<https://github.com/KenM76/swformat>, `docs/LEGAL.md`),
> with two statements corrected for accuracy (see §4 and §2 notes).
> Legal or IP questions: open an issue on the OxyBrush repository.

---

## 1. What this work is

`oxy-ztl` is a component of OxyBrush that aims to **read** (and, much later
and only if verified safe, **write**) files in ZBrush's native formats
(`.ZTL` tools, `.ZPR` projects) so artists can move their own work between
ZBrush and an independently created, open-source sculpting application.

It is built only from:

- Documentation produced by analysing the bytes of **legitimately obtained
  files** (files the analyst created, or has the right to use).
- License-compatible open-source prior art, if any is used (attributed in
  `research/<source>_notes/README.md`; MIT/BSD/Apache-2.0 only).
- Rust's standard library and permissively licensed crates.

The work **does NOT**:

1. **Decrypt** any encrypted or obfuscated-for-protection ZBrush data. If a
   file or region turns out to be encrypted, it is detected and reported as
   unsupported, and analysis of that region stops.
2. **Circumvent** any technical protection measure (licensing, DRM,
   activation, signatures).
3. **Replicate or replace** the ZBrush application.
4. **Include, redistribute, or derive from** ZBrush source code or any
   ZBrush/Maxon proprietary binary, plug-in, brush, material, tool or
   sample file.

Format knowledge is derived from **file contents** (diff pairs, see
`REVERSE_ENGINEERING.md`). Disassembling, decompiling or debugging the
ZBrush program is not part of this process and must not be contributed.

## 2. Legal basis: interoperability

The work relies on the interoperability provisions found in copyright law:

### United States

- **17 U.S.C. § 1201(f)** — exempts from the DMCA's anti-circumvention
  rules acts done "for the sole purpose of identifying and analyzing those
  elements of the program that are necessary to achieve interoperability of
  an independently created computer program with other programs."
  *Accuracy note:* §1201(f) is a narrow exemption to the anti-circumvention
  provisions, not a general grant of permission to reverse engineer. This
  project does not circumvent anything (§1), so its main relevance is as a
  statement of the policy favouring interoperability.
- **Fair use / case law** — analysing a program's outputs or intermediate
  copies for interoperability has been held to be fair use (*Sega v.
  Accolade*, 977 F.2d 1510 (9th Cir. 1992); *Sony v. Connectix*, 203 F.3d
  596 (9th Cir. 2000)). File formats as such are generally treated as
  functional and not protected by copyright.

### European Union

- **Directive 2009/24/EC, Article 6** — permits reproduction/translation of
  code where indispensable to obtain interoperability information, under
  conditions (lawful user; information not readily available; confined to
  what is necessary). Article 8 makes contract terms contrary to Article 6
  null and void.

### Other jurisdictions

Comparable provisions exist in, e.g., UK CDPA 1988 §50B, Canada Copyright
Act §30.61, Australia Copyright Act §47D. Scope differs by country.

## 3. User expectations

`oxy-ztl` is intended for users who:

1. **Have legitimate access to the files they process** (typically their own
   sculpts).
2. Use it for **interoperability**: opening their own work in OxyBrush.

## 4. Relationship to the ZBrush EULA

The ZBrush license agreement, like most commercial EULAs, may restrict
reverse engineering.

- In the **EU**, contract terms that prohibit what Article 6 permits are void
  (Directive 2009/24/EC Art. 8).
- In the **US the situation is unsettled**: courts have enforced
  contractual no-reverse-engineering clauses (*Bowers v. Baystate
  Technologies*, 320 F.3d 1317 (Fed. Cir. 2003); *Davidson & Associates v.
  Jung*, 422 F.3d 630 (8th Cir. 2005)). *Accuracy note:* this corrects the
  SWFormat document, which states such clauses are unenforceable in the US.
- Contributors who have accepted a ZBrush EULA are personally responsible
  for complying with it. This is one reason the methodology analyses only
  **file bytes**, never the ZBrush program.

Contributors and users in any jurisdiction should evaluate their own
situation; the project maintainers will seek advice from an open-source
legal resource before the first release that includes `oxy-ztl` reading
support.

## 5. Contributions and corpus IP rules

**Contributions MUST NOT include:**

- ZBrush/Maxon proprietary code, binaries, plug-ins, or bundled content
  (default tools, brushes, materials, alphas, project files).
- Client or employer work-product files without explicit permission.
- Files acquired through unauthorised means.
- Findings obtained by disassembling, decompiling or debugging ZBrush.

**Corpus rules (enforced by `.gitignore` and CI, see `test/corpus/zbrush/`):**

- ZBrush files (`*.ztl`, `*.zpr`, `*.zbr`, `*.zsc`, `*.zmt`, any case) are
  blocked from commit by default.
- Only files with `license_status: synthetic` in `corpus.config.json` —
  i.e. trivially simple files the contributor created from scratch and
  dedicates as CC0 — may be committed, each with an explicit `.gitignore`
  negation and a matching config entry.
- Real files (`license_status: owner`) are referenced by **absolute local
  path** and never copied into the repository.
- CI fails the build if a ZBrush file is committed without a matching
  `synthetic` entry.

**Reference prior art MUST** be license-compatible with MIT OR Apache-2.0 and
be attributed.

## 6. Patents

The project takes no position on patents. Users assume their own risk.

## 7. Trademarks

ZBrush is a registered trademark of Maxon Computer GmbH (formerly
Pixologic, Inc.). `.ZTL`/`.ZPR` and related marks belong to their
respective owners.

OxyBrush and `oxy-ztl` are **independent, unofficial** projects, **not
affiliated with, authorized by, endorsed by, or sponsored by** Maxon or any
of its affiliates. References to ZBrush are **nominative** — used only to
identify the file formats this project interoperates with.

## 8. Contact

Legal or IP correspondence: open an issue on the OxyBrush repository.
Maxon representatives: please reach out via the repository before sending a
takedown notice; the maintainers will respond promptly to clarify or amend.

## 9. Disclaimer

This document describes the project's intended posture and is not legal
advice. Consult a qualified attorney in your jurisdiction for certainty
about your own exposure. The software is provided without warranty under
MIT OR Apache-2.0.
