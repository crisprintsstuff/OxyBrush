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
