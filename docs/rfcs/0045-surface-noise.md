# RFC 0045 — Surface Noise

- **Status:** Phase 1 implemented
- **Issue:** [#541](https://github.com/crisprintsstuff/OxyBrush/issues/541)
- **Name in UI:** **Surface Noise** (generic; not a third-party product name)

## Problem

Artists want whole-piece procedural surface detail — pores, grit, soft
undulation — with scale and strength they can dial in, then bake to
geometry. OxyBrush already has:

- a **Noise brush** (stroke-local),
- **Deform > Noise** (whole piece, fixed frequency, amount bar only).

Neither is a dedicated surface-noise panel with scale, strength, pattern
choice and a clear Apply step.

## Decision

Add **Tools > Surface Noise…** (command palette too): a window that
displaces the active piece along its normals using 3D procedural noise.

### Phase 1 controls

| Control | Meaning |
|---|---|
| Scale | Feature size relative to the piece (larger = broader bumps) |
| Strength | Displacement amplitude relative to the piece size |
| Octaves | 1–8; higher adds finer detail (FBM) |
| Seed | Changes the pattern |
| Pattern | Smooth (value), Turbulent (abs FBM), Cellular (Worley) |
| Use mask | Masked points stay put (default on) |
| Apply | Bake the current preview as one undo step |

Live preview while parameters change (from a snapshot of the piece when
the window opened or after the last Apply). Closing without Apply restores
the snapshot.

### Placement

- Math in **`oxy-mesh`** (`surface_noise` module): GPU-free, pure Rust.
- App: `surface_noise_ui` window, same preview/undo patterns as Deform /
  Color Filter.
- Do **not** copy other products' UI layouts, icons or noise presets.
  Behavior informed only by the public Maxon Surface Noise help page and
  Discord request (#541) — see [CLEAN_ROOM.md](../CLEAN_ROOM.md).

### Naming

UI says **Surface Noise**. Do not use proprietary product names for the
feature. Naming ZBrush only when identifying the issue request is fine.

## Non-goals (Phase 1)

- Non-destructive viewport-only bump (always bake on Apply)
- Mix stacks of several noises
- Noise-as-brush-setting for every brush
- Image / alpha-driven noise maps (later)
- Curve editor for remapping the noise response (later)

## Phases

| Phase | Ships |
|---|---|
| **1** | Window; scale / strength / octaves / seed / pattern; mask; live preview; Apply undo |
| **2** | Response curve; offset gizmo; optional color blend from noise |
| **3** | Mix layers; noise as optional brush modulation |

## Verification

- Unit tests for noise apply (masked verts unchanged, strength 0 = identity)
- Editor-level open / apply / undo test
- AGENTS checks before push
