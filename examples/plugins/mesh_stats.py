"""
# Name: Mesh Inspector
# Description: Prints vertex, triangle counts, piece information, and bounds
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
    app = oxybrush.attach()
    scene = app.scene_info()
    pieces = app.list_pieces()

    print(f"=== OxyBrush Scene Inspector ===")
    print(f"Total pieces: {len(pieces)}")
    for p in pieces:
        status = " (active)" if p.get("is_active") else ""
        print(f"Piece #{p.get('id')}: '{p.get('name')}' - {p.get('triangles', 0):,} tris{status}")


if __name__ == "__main__":
    main()
