# AGENTS.md — instructions for AI coding agents

This file tells AI agents (Claude Code, Codex, Copilot, Cursor and others)
how to work on OxyBrush. Humans should read [CONTRIBUTING.md](CONTRIBUTING.md);
everything here is consistent with it, and where the two differ,
CONTRIBUTING.md and the documents it links win.

## Project in one paragraph

OxyBrush is an open-source digital sculpting application in Rust
(MIT OR Apache-2.0). Supported platforms: Windows 10/11, Linux (X11/Wayland)
and macOS 11+ (universal: Apple Silicon and Intel). Current status and plan:
[docs/ROADMAP.md](docs/ROADMAP.md). Design rationale:
[docs/rfcs/0001-oxybrush.md](docs/rfcs/0001-oxybrush.md).

## Repository layout

```
crates/
  oxy-mesh     chunked triangle mesh, face groups/visibility, raycast, subdivision   (no GPU)
  oxy-sculpt   stroke engine, brush kernels, symmetry, falloff                       (no GPU)
  oxy-topo     voxel remesh, quadric decimation                                     (no GPU)
  oxy-doc      scenes of pieces, .oxb format, autosave/recovery                     (no GPU)
  oxy-history  scene-wide undo/redo                                                 (no GPU)
  oxy-io       OBJ/STL import/export                                                (no GPU)
  oxy-ztl      ZBrush format research tools (see "ZBrush rules" below)              (no GPU)
  oxy-render   wgpu matcap renderer, camera
  oxy-app      winit + egui desktop app, binary `oxybrush`
docs/          ROADMAP.md, rfcs/, formats/zbrush/, releases/<tag>.md, images/
packaging/     icons/ (generated), windows/ (Inno Setup), flatpak/, macos/ (app bundle + DMG)
scripts/       check_corpus_ip.py (CI guard)
test/corpus/   ZBrush corpus registry template (never ZBrush files)
```

## Commands

The toolchain is pinned in `rust-toolchain.toml` (currently 1.99.1); rustup
installs it automatically. Run these before every commit; CI runs the same:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/check_corpus_ip.py
```

Useful extras:

```bash
cargo check --workspace --target x86_64-pc-windows-gnu     # Windows type-check from Linux (rustup target add first)
cargo run -p oxy-app -- --demo --screenshot out.png        # headless screenshot (Linux: xvfb-run + mesa-vulkan-drivers)
cargo run --release -p oxy-sculpt --example dab_timing     # brush performance
cargo run --release -p oxy-topo --example topo_timing      # remesh/decimate performance
cargo build --profile dist -p oxy-app                      # stripped release binary as shipped
```

Look at the headless screenshot whenever you change rendering or UI; tests
alone don't show visual regressions.

## Architecture invariants (do not break)

1. **GPU-free core.** Everything except `oxy-render` and `oxy-app` must build
   and test headless, with no wgpu/winit/egui dependency.
2. **Chunked mesh.** Each vertex is owned by exactly one chunk, and chunk
   vertex ranges are contiguous. Brushes must touch only affected chunks and
   call `Mesh::update_after_move` with the vertices they moved; undo snapshots
   chunks via `History::touch` **before** modifying them.
3. **Topology/face versions.** Anything that changes triangles goes through
   `Mesh::from_data`/`with_mask`; face group or visibility edits go through
   `set_groups`/`set_hidden`. These bump the versions the renderer relies on.
4. **Undo for every user-visible change.** Strokes use begin/touch/commit;
   topology uses `record_topology`; face edits use `record_faces`; piece
   add/remove/merge use `record`/`record_compound`.
5. **Long operations run off the UI thread** (see `Editor::start_job`).
6. **Never lose user data.** Writes are atomic (temp file + rename); never
   overwrite an input file in place; keep autosave/recovery working.
7. **`.oxb` compatibility.** The format is versioned with skippable sections.
   Add new data as new sections; never change the meaning of existing ones
   without bumping the version and keeping the old reader path.

## Hard rules

- **No `unsafe`.** Forbidden workspace-wide.
- **Pure Rust.** No dependencies that need a C/C++ toolchain on Windows,
  Linux or macOS (that's why decimation is hand-written instead of using meshoptimizer).
  The only exception is `winresource`, a Windows-only build dependency that
  degrades gracefully.
- **Licenses.** Dependencies and ported code must be MIT/Apache-2.0/BSD/
  zlib-compatible. **Never copy GPL code**, including Blender's; studying
  behavior and published papers is fine (cite the paper in a comment).
- **Clean-room behavior.** Never consult leaked or decompiled proprietary
  code. Don't copy other products' UI layouts, icons, materials, brushes or
  other assets; everything shipped is original or CC0/CC-BY.
- **Trademarks.** Don't use ZBrush/Maxon proprietary feature names (DynaMesh,
  ZRemesher, ZSpheres, SubTool, Polypaint, …) for OxyBrush features, in code,
  UI or docs. Use generic names: voxel remesh, auto-retopology, armature
  spheres, pieces, vertex color. Naming ZBrush only to identify its file
  format or to compare features is fine.
- **No telemetry or network calls** in the app.
- **Performance is a feature.** Don't add work to the per-dab or per-frame
  path without measuring (`dab_timing`). Report numbers when you touch hot
  paths.

## ZBrush rules (crates/oxy-ztl, docs/formats/zbrush)

Read [docs/formats/zbrush/LEGAL.md](docs/formats/zbrush/LEGAL.md) and
[REVERSE_ENGINEERING.md](docs/formats/zbrush/REVERSE_ENGINEERING.md) before
touching this area. In particular:

- **Never commit ZBrush files** (`.ztl .zpr .zbr .zsc .zmt`, any case). CI
  fails if you do. Real files are referenced by absolute path in a private
  corpus registry (`OXY_ZTL_CORPUS`), never copied into the repo.
- Analyse **file bytes only**. Never disassemble, decompile or debug the ZBrush
  program, and never decrypt or circumvent any protection. If data appears
  encrypted, stop and record the finding.
- Every format claim needs a diff-pair experiment, a hypothesis-log entry and
  falsification checks before it becomes a lesson and then code. Don't
  invent format details: if it isn't in a lesson, it isn't known.
- Keep the tail-bytes and no-orphan-bytes invariants (every byte read is
  written back verbatim).
- Comment for two audiences: prose explaining *why*, plus explicit
  byte-offset tables and `INVARIANT:`/`PRECONDITION:`/`RETURNS:` labels.

## Workflow

- **Branch and pull request.** Never push to `main`. Work on a feature
  branch, open a pull request, and let a maintainer merge it. Don't merge or
  approve pull requests yourself unless the maintainer explicitly asks.
- **Sign off** every commit (`git commit -s`, Developer Certificate of
  Origin). Explain *why* in the message body.
- **Keep docs current in the same change:** update
  [docs/ROADMAP.md](docs/ROADMAP.md) (status, checkmarks, "Last updated")
  when a feature lands, the README when user-visible behavior or controls
  change, and the in-app shortcut list (`SHORTCUTS` in
  `crates/oxy-app/src/ui.rs`) when shortcuts change.
- **Tests with every change:** unit tests in the crate you changed; editor-level
  tests in `crates/oxy-app/src/editor.rs` for interaction changes.
- **Large or user-facing design changes** start as an RFC in `docs/rfcs/`.
- **Be honest in summaries and pull request descriptions:** say what was
  verified (and how), what wasn't, and known limitations. Never claim
  something works on Windows, on a tablet or on a GPU unless it was actually
  run there.
- If CI fails because a new Rust release added lints, fix the code and bump
  `rust-toolchain.toml` deliberately in a separate commit.

## Releases

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`).
2. Add `docs/releases/v<version>.md` (it becomes the release notes) and update
   `docs/ROADMAP.md`, plus the `<releases>` entry in
   `packaging/flatpak/io.github.crisprintsstuff.OxyBrush.metainfo.xml`.
3. Test packaging by running the **Release builds** workflow manually
   (workflow_dispatch) on the branch.
4. After merge, a maintainer publishes a GitHub release with tag
   `v<version>`. The workflow checks the tag matches `Cargo.toml`, builds the
   Windows installer/zip, Linux Flatpak/tarball and macOS universal DMG, and
   attaches them with `SHA256SUMS.txt`. Tags containing `-` are marked
   pre-release.

Regenerate icons with `python3 packaging/icons/make_icons.py` if the design
changes; never hand-edit the generated PNG/ICO files.

## Environment notes for cloud agents

- Some automation environments cannot create git tags or GitHub releases, or
  use GitHub's GraphQL API. Use the REST API for pull requests, and ask the
  maintainer to publish releases (step 4 above). Don't work around blocked
  operations by other routes.
- `xvfb-run` plus `mesa-vulkan-drivers` (lavapipe) is enough to run the app
  headless for screenshots.
