# Vendored dependencies

## winit 0.30.13 (Apache-2.0)

A copy of [winit](https://github.com/rust-windowing/winit) 0.30.13 from crates.io
(examples, tests and docs removed), used through `[patch.crates-io]` in the root
`Cargo.toml`. Its license is in `winit/LICENSE`.

**The only change** is in `src/platform_impl/windows/event_loop.rs`, marked
`OxyBrush patch`: with Windows Ink, winit turns pen input into touch events
and drops the pen's button flags, so a pen touching down with its barrel (side)
button held looked exactly like a sculpting stroke
([#59](https://github.com/crisprintsstuff/OxyBrush/issues/59)). The patch
reports such a pen contact as the right mouse button instead (press, moves,
release), so the user's navigation preset handles it.

Drop this copy when OxyBrush moves to a winit release that reports pen buttons
itself (winit 0.31 has pen input).
