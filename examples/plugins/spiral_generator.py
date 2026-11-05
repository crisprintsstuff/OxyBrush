"""
# Name: Spiral Generator
# Description: Generates a smooth 3D spiral curve sculpted onto the model
"""

import math
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
