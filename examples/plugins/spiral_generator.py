"""
# Name: Spiral Generator
# Description: Generates a smooth 3D spiral curve sculpted onto the model
"""

import math
import os
import sys
from pathlib import Path

# If oxybrush is not in site-packages, search OxyBrush application data directories
if "oxybrush" not in sys.modules:
    for candidate in [
        Path(os.environ.get("LOCALAPPDATA", "")) / "OxyBrush" / "python",
        Path(os.environ.get("APPDATA", "")) / "OxyBrush" / "python",
        Path.home() / "AppData" / "Local" / "OxyBrush" / "python",
        Path.home() / ".local" / "share" / "oxybrush" / "python",
        Path.home() / "Library" / "Application Support" / "oxybrush" / "python",
        Path(__file__).resolve().parent.parent / "python",
        Path(__file__).resolve().parent.parent.parent / "python",
    ]:
        if (candidate / "oxybrush" / "__init__.py").is_file() and str(candidate) not in sys.path:
            sys.path.insert(0, str(candidate))
            break

import oxybrush


def main():
    app = oxybrush.attach()
    print("Connected to OxyBrush!")

    # Configure brush
    app.select_brush("Clay")
    app.set_brush(size=40.0, strength=0.5)

    # Generate a spiral of points around the origin
    points = []
    steps = 100
    for i in range(steps):
        t = (i / float(steps)) * 4.0 * math.pi
        r = 1.0 + 0.1 * math.sin(t * 3.0)
        x = math.cos(t) * r
        y = (i / float(steps) - 0.5) * 1.8
        z = math.sin(t) * r
        points.append([x, y, z])

    res = app.stroke(points=points, space="world")
    print(f"Stroke result: {res}")


if __name__ == "__main__":
    main()
