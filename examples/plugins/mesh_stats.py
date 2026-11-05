"""
# Name: Mesh Inspector
# Description: Prints vertex, triangle counts, piece information, and bounds
"""

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
