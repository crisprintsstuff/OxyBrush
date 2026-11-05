#!/usr/bin/env python3
"""
# Name: Mesh Stats Reporter
# Description: Prints vertex count, triangle count, and piece metrics
# Author: OxyBrush Team
# Version: 1.0.0
"""

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
    session = oxybrush.attach()
    stats = session.get_mesh_stats()

    print(f"{stats.piece_count} piece(s) | {stats.triangle_count:,} tris | {stats.vertex_count:,} verts")


if __name__ == "__main__":
    main()
