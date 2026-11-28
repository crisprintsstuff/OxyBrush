# RFC 0060 — Screenshot export

- **Status** · Implemented in 4.2.1
- **Scope** · Save the current sculpting view as an image from the desktop app.

## Workflow

Open **File > Export screenshot…**. Choose **Viewport** for the sculpt and
its rendered background, or **Full window** to include the OxyBrush interface
when reporting a bug. Choose **PNG**, **JPEG**, or **WebP**, then click
**Save image…** to select a destination.

## Capture

Use the current viewport resolution, with both panes included when split view
is on. Viewport capture reads the rendered frame before the interface is
drawn. Full-window capture reads after it is drawn, once the export dialog has
closed. PNG and WebP keep the original colors losslessly. JPEG uses quality
90. The export uses the current background and lighting.

Reuse the desktop screenshot readback and encode and write the image on a
worker. Report success or a write failure in the status bar. Image export
leaves the project and its undo history untouched.

## Acceptance

- Linux, Windows, and macOS expose the same options through File.
- Every format produces a readable image with the chosen capture area.
- Cancelling either dialog leaves existing files intact.
