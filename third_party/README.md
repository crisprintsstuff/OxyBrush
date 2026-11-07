# Vendored dependencies

## winit 0.30.13 (Apache-2.0)

A copy of [winit](https://github.com/rust-windowing/winit) 0.30.13 from crates.io
(examples, tests and docs removed), used through `[patch.crates-io]` in the root
`Cargo.toml`. Its license is in `winit/LICENSE`.

There are two changes, each marked `OxyBrush patch`.

**Windows pen side button** (`src/platform_impl/windows/event_loop.rs`): with Windows Ink, winit turns pen input into touch events
and drops the pen's button flags, so a pen touching down with its barrel (side)
button held looked exactly like a sculpting stroke
([#59](https://github.com/crisprintsstuff/OxyBrush/issues/59)). The patch
reports such a pen contact as the right mouse button instead (press, moves,
release), so the user's navigation preset handles it.

**macOS Quit** (`src/platform_impl/macos/app_state.rs`): winit's application
delegate has no `applicationShouldTerminate:`, so Quit in the app menu (Cmd+Q),
Quit in the Dock and logging out ended the process without the application
hearing about it, and unsaved work was lost
([#69](https://github.com/crisprintsstuff/OxyBrush/issues/69)). The patch turns
the request into a `CloseRequested` for each winit window and cancels the
termination, so OxyBrush can ask to save and then exit its event loop. Once the
event loop is exiting, or when there is no window, macOS terminates as before.

Drop this copy when OxyBrush moves to a winit release that reports pen buttons
itself (winit 0.31 has pen input) and lets the application veto Quit on macOS.
