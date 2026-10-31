# OxyBrush

**An open-source digital sculpting application written in Rust.**

OxyBrush is for sculptors: concept artists, character artists, miniature and 3D-print makers, and students. The model fills the screen, tools show up when you call for them, and brushes respond immediately even on dense meshes.

![OxyBrush sculpting a sphere with Clay, Draw, Inflate and Mask strokes](docs/images/screenshot.png)

> **Status: early development (Phase 0–1, tech preview).** You can sculpt, undo, subdivide, mask and export today. Expect rough edges, and don't rely on it for production work yet. See the [roadmap](#roadmap).

## Features (current)

- **7 brushes:** Draw, Clay, Smooth, Inflate, Flatten, Grab, Mask, with falloff curves, spacing, auto-smooth and pen-pressure options.
- **Symmetry:** X/Y/Z mirroring (X is on by default).
- **Chunked mesh engine:** a stroke only re-computes, re-uploads and snapshots the regions it touches.
- **Undo/redo:** unlimited within a memory budget, using per-chunk snapshots.
- **Topology:** subdivide (linear or smoothed). Start shapes: sphere, cube, plane.
- **Masking:** paint (Ctrl), unmask (Ctrl+Alt), invert, clear, show/hide.
- **Matcap viewport (wgpu):** five procedurally generated materials (no third-party assets).
- **Import/export:** OBJ and STL import (welded for sculpting). OBJ and binary STL export (3D-printing ready).
- **Accessibility basics:** UI scaling (75–300%), a high-contrast theme, a full mouse-only workflow, and screen-reader labels through AccessKit.

## Performance status

Measured with `cargo run --release -p oxy-sculpt --example dab_timing` on a 4-core cloud VM (CPU only), using a 1.3M-triangle sphere:

| Dab | Time |
|---|---|
| Small (~1k vertices) | ~0.7 ms |
| Large (~15k vertices) | ~2 ms |

The 1.0 target (≤8 ms per dab at ~100k vertices) still needs work: smarter chunk refits, SIMD kernels, and measurement on the reference hardware.

## Building

You need Rust 1.88 or newer ([rustup](https://rustup.rs)).

```bash
git clone https://github.com/crisprintsstuff/OxyBrush
```

```bash
cd OxyBrush
```

```bash
cargo run --release
```

Open a mesh on launch:

```bash
cargo run --release -- path/to/model.obj
```

**Linux packages:** X11 or Wayland development libraries and a Vulkan driver. On Debian or Ubuntu:

```bash
sudo apt install libxkbcommon-x11-0 libwayland-dev mesa-vulkan-drivers
```

**macOS and Windows:** nothing extra (Metal and DX12/Vulkan are used automatically).

## Controls

| Input | Action |
|---|---|
| LMB on model | Sculpt with the active brush |
| LMB on empty space | Orbit |
| Alt+LMB on empty space / RMB | Pan |
| Wheel | Zoom |
| Shift / Ctrl / Alt (hold) | Smooth / Mask / Invert |
| `1`–`7` | Select brush |
| `[` `]` and `-` `=` | Radius and strength |
| `X` | Toggle X symmetry |
| `F` | Frame model |
| `D` | Subdivide |
| Ctrl+Z, Ctrl+Shift+Z | Undo, redo |
| `F1` | Full shortcut list |

## Architecture

```
crates/
  oxy-mesh     chunked triangle mesh, adjacency, raycast, primitives, subdivision  (no GPU)
  oxy-sculpt   stroke engine, brush kernels, symmetry, falloff                     (no GPU)
  oxy-history  undo/redo via chunk snapshots and a memory budget                   (no GPU)
  oxy-io       OBJ/STL import and export                                           (no GPU)
  oxy-render   wgpu matcap renderer and orbit camera
  oxy-app      winit + egui desktop app (binary: `oxybrush`)
```

Every crate except `oxy-render` and `oxy-app` runs headless and is unit-tested. The full design is in [RFC-0001](docs/rfcs/0001-oxybrush.md).

## Roadmap

| Phase | Goal | State |
|---|---|---|
| 0 | Foundations: workspace, CI, renderer, mesh core | ✅ |
| 1 | Sculpt core: brushes, symmetry, undo, pressure | 🚧 mostly done; tablet pressure on all OSes still open |
| 2 | Voxel remesh, decimation, multiple pieces, face groups, `.oxb` project files, autosave | ⏳ |
| 3 | Quick palette, keymap editor, accessibility audit, installers | ⏳ |
| 4 | Hardening → **1.0** | ⏳ |
| 1.x / 2.x | Multires, dynamic topology, layers, vertex color, plugin API, and **read-only ZBrush `.ZTL` import** (see [interop policy](docs/formats/zbrush-interop.md)) | long-term |

## Contributing

Everyone is welcome, whether you write code, make art, write docs, translate, or test. Read [CONTRIBUTING.md](CONTRIBUTING.md) first. In particular, every commit needs a DCO sign-off (`git commit -s`), and there is a **clean-room policy**.

## License

Code is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Bundled assets are CC0.

*OxyBrush is not affiliated with or endorsed by Maxon or Pixologic. ZBrush is a trademark of Maxon Computer GmbH. It is mentioned only to describe planned file compatibility.*
