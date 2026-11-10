# Vendored dependencies

## winit 0.30.13 (Apache-2.0)

A copy of [winit](https://github.com/rust-windowing/winit) 0.30.13 from crates.io
(examples, tests and docs removed), used through `[patch.crates-io]` in the root
`Cargo.toml`. Its license is in `winit/LICENSE`.

There are four changes, each marked `OxyBrush patch`.

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

**macOS tablet pressure** (`src/platform_impl/macos/view.rs`): winit 0.30
reports pressure on macOS only for Force Touch trackpads. Drawing tablets
deliver pen pressure inside mouse events with the tablet-point subtype (or as
tablet-point events), and winit dropped it, so pressure never reached
OxyBrush ([#70](https://github.com/crisprintsstuff/OxyBrush/issues/70)). The
patch reports it as `TouchpadPressure`, before and after each pointer move,
and handles `tabletPoint:` events. Only mouse and tablet-point events are
inspected (asking another kind of event for its subtype raises an
Objective-C exception).

**X11 tablet pressure** (`src/platform_impl/linux/x11/mod.rs` and
`event_processor.rs`): winit reports a pen's XInput2 pressure valuator as an
`AxisMotion` with the device's raw reading (0..=2047, 0..=8191 or 0..=65535,
depending on the tablet), with no way for the application to know which axis
is pressure or its range. OxyBrush only accepted values between 0 and 1, so on
most tablets pressure never took effect on Linux
([#81](https://github.com/crisprintsstuff/OxyBrush/issues/81)). The patch
finds the valuator labelled "Abs Pressure" when a device is added, keeps its
range, and reports its value as `TouchpadPressure` scaled to 0..=1, the same
event the macOS patch uses.

Drop this copy when OxyBrush moves to a winit release that reports pen buttons
itself (winit 0.31 has pen input) and lets the application veto Quit on macOS.
