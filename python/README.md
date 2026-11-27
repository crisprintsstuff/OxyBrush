# OxyBrush Python SDK

The official Python scripting and plugin library for [OxyBrush](https://github.com/crisprintsstuff/OxyBrush).

Use it to automate sculpting, generate procedural meshes, create custom tools, and build OxyBrush plugins without needing C compilation or unsafe dependencies.

## Installation

When running plugins from inside OxyBrush, this SDK is made available automatically.

To use it in your own external Python scripts, virtual environments, or Blender:

```bash
pip install -e /path/to/OxyBrush/python
```

or install directly from GitHub:

```bash
pip install "git+https://github.com/crisprintsstuff/OxyBrush.git#subdirectory=python"
```

## Quick Start

### 1. In-App Plugin

Create a `.py` file in your OxyBrush plugins folder (opened via **Plugins > Open plugins folder…**):

```python
"""
# Name: Spiral Ribbons
# Description: Sculpt procedural spiral strokes around the active model
"""
import math
import oxybrush

# Connects to the running OxyBrush session
app = oxybrush.attach()

# Select a brush and configure it
app.select_brush("Clay")
app.set_brush(size=35.0, strength=0.6)

# Generate stroke points
points = []
for i in range(120):
    t = i / 120.0 * 6.0 * math.pi
    radius = 1.0 + 0.15 * math.sin(t * 2)
    x = math.cos(t) * radius
    y = (i / 120.0 - 0.5) * 2.0
    z = math.sin(t) * radius
    points.append([x, y, z])

app.stroke(points=points, space="world")
print("Procedural spiral sculpt complete!")
```

Run it directly from **Plugins > Run Python script…** or click it in the Plugins menu.

### 2. Standalone Script

```python
import oxybrush

# Connects to a running instance (or default port)
app = oxybrush.connect()

# Query scene info
info = app.scene_info()
print(f"Pieces: {len(info.get('pieces', []))}")

# Add a primitive and remesh
app.add_primitive("cube")
app.remesh(resolution=128)

# Undo / Redo
app.undo()
```

## Geometry edits

Use `get_geometry` to read a piece's vertex order and topology version.
`patch_geometry` sets absolute positions, colors, or mask values at selected
indices. Consecutive writes to the same piece with the same `undo_group` can
be undone together. For example, paint two vertices on the active piece:

```python
mesh = app.get_geometry(fields=["positions"])
target = {"id": mesh["piece"], "topology_version": mesh["topology_version"]}

app.patch_geometry([0], colors=[[0.7, 0.2, 0.1]], undo_group="vertex-color", **target)
app.patch_geometry([1], colors=[[0.2, 0.5, 0.8]], undo_group="vertex-color", **target)
```

Read the geometry again if a topology change invalidates the version. The same
grouping option works with dense `set_geometry` writes. In a change listener,
compare a geometry event's `origin` with the `session_origin` returned by
`changes()` to recognize your session's writes.

## Desktop render previews

In a single desktop viewport, `get_viewport_state()` returns the physical
pixel dimensions, camera, and `view_token`. Read `scene_state()["lighting"]`
to match the app's lighting rig. Render to a local file containing raw
straight-alpha sRGB RGBA8 pixels in row order. The file must be exactly
`width * height * 4` bytes and at most 64 MiB. Pass the file to
`set_viewport_image(path, view_token=state["view_token"])` using the state
from before rendering. The preview clears when the view or scene changes.
Use `clear_viewport_image()` to remove it manually.

[Geometry write and viewport API details](../docs/rfcs/0053-mcp-geometry-write.md)
