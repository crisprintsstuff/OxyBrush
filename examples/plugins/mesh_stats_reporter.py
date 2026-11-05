#!/usr/bin/env python3
"""
# Name: Mesh Stats Reporter
# Description: Prints vertex count, triangle count, and memory statistics
# Author: OxyBrush Team
# Version: 1.0.0
"""

import oxybrush


def main():
    session = oxybrush.attach()
    stats = session.get_mesh_stats()

    print(f"{stats.piece_count} piece(s), {stats.triangle_count:,} tris, {stats.vertex_count:,} verts")


if __name__ == "__main__":
    main()
