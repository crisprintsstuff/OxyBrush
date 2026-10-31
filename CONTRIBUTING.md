# Contributing to OxyBrush

Thank you for helping! This guide covers how to contribute and the project's legal ground rules.

## Ways to help

- **Code:** look for issues labeled `good first issue`.
- **Art and testing:** try the app, report how brushes *feel*, and contribute CC0 test meshes.
- **Docs and translations:** user guide pages, tutorials, translations (planned with Fluent).
- **Design:** UX proposals go through an RFC (below).

## Development workflow

```bash
cargo test --workspace
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo fmt --all
```

- Keep algorithm crates (`oxy-mesh`, `oxy-sculpt`, `oxy-history`, `oxy-io`) **free of GPU and windowing dependencies**.
- Any change that adds stroke latency or frame time is a regression. Include numbers when you touch hot paths.
- `unsafe` is forbidden workspace-wide. Proposals to relax that need an RFC.
- Headless screenshot (used in CI and docs):

```bash
cargo run -- --demo --screenshot out.png
```

## Sign-off (DCO)

Every commit must be signed off, which certifies the [Developer Certificate of Origin](https://developercertificate.org/):

```bash
git commit -s
```

There is no CLA.

## Clean-room policy

OxyBrush takes inspiration from how sculpting tools *feel*, never from their code or assets.

1. **Never** consult leaked, decompiled or disassembled proprietary code (ZBrush or any other).
2. Do not copy third-party UI layouts, icons, materials, brushes or other assets. Everything we ship is original, CC0 or CC-BY 4.0.
3. Do not use ZBrush or Maxon trademarks or proprietary feature names (for example DynaMesh, ZRemesher, ZSpheres, SubTool, Polypaint) in code, the UI or docs. Use the generic terms: voxel remesh, auto-retopology, armature spheres, pieces, vertex color.
4. Blender is GPL. OxyBrush is MIT/Apache-2.0, so **do not copy Blender code**. Studying behavior and the published papers behind it is fine. Cite papers in code comments.
5. Proprietary file formats (for example ZBrush `.ZTL`/`.ZPR`) are out of scope. Please don't submit reverse-engineering work for them. Interchange uses open formats.

## RFCs

Changes that affect users, file formats or public APIs start as a short RFC in `docs/rfcs/`. Copy the structure of RFC-0001, open a PR, and allow at least a week for discussion. Artist feedback carries real weight in UX RFCs.

## Code of conduct

All participation is governed by our [Code of Conduct](CODE_OF_CONDUCT.md).
