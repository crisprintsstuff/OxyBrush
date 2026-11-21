# SPDX-License-Identifier: GPL-3.0-or-later
"""Run in background Blender with the bridge extension installed and enabled."""

import importlib
import json
from pathlib import Path
from tempfile import TemporaryDirectory

import bpy
from mathutils import Vector


module = next(name for name in bpy.context.preferences.addons.keys() if name.endswith(".oxybrush_bridge"))
bridge = importlib.import_module(module)
cube = bpy.context.active_object
mesh = cube.data
basis = cube.shape_key_add(name="Basis")
shape = cube.shape_key_add(name="Test shape")
shape.data[0].co.y += 0.125
shape.value = 1.0
offset = Vector((0.25, 0.0, 0.0))
positions = [vertex.co + offset for vertex in mesh.vertices]
material = bpy.data.materials.new("Bridge Material")
mesh.materials.append(material)
cube.vertex_groups.new(name="Bridge Group").add([0], 1.0, "REPLACE")
cube.modifiers.new("Bridge Modifier", "BEVEL").show_viewport = False


def write_obj(path, verts, faces):
    lines = ["v %.9g %.9g %.9g" % tuple(vertex) for vertex in verts]
    lines.extend("f " + " ".join(str(i + 1) for i in face) for face in faces)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


with TemporaryDirectory() as directory:
    folder = Path(directory)
    manifest = folder / "returned.json"
    write_obj(folder / "cube.obj", positions, [polygon.vertices for polygon in mesh.polygons])
    write_obj(folder / "piece.obj", positions[:3], [[0, 1, 2]])
    original = {"name": cube.name, "file": "cube.obj", "same_topology": True}
    new = {"name": "New piece", "file": "piece.obj", "new": True}
    manifest.write_text(json.dumps({"objects": [original, new]}), encoding="utf-8")
    link = {"objects": {cube.name: cube.name}, "new_objects": {}}
    assert not bridge._apply(directory, link)
    assert cube.data == mesh
    assert mesh.uv_layers and mesh.materials[-1] == material
    assert cube.vertex_groups["Bridge Group"].weight(0) == 1.0
    assert cube.modifiers.get("Bridge Modifier")
    assert all(point.co == pos for point, pos in zip(basis.data, positions))
    shape_offset = Vector((0.0, 0.125, 0.0))
    assert shape.data[0].co == positions[0] + shape_offset
    bpy.context.view_layer.update()
    evaluated = cube.evaluated_get(bpy.context.evaluated_depsgraph_get())
    assert evaluated.data.vertices[0].co == positions[0] + shape_offset

    piece = bpy.data.objects["New piece"]
    count = len(bpy.context.scene.objects)
    write_obj(folder / "cube.obj", positions[:3], [[0, 1, 2]])
    original["same_topology"] = False
    manifest.write_text(json.dumps({"objects": [original, new], "closed": True}), encoding="utf-8")
    assert bridge._apply(directory, link)
    assert cube.data != mesh and len(cube.data.vertices) == 3
    assert cube.data.materials[-1] == material
    assert bpy.data.objects["New piece"] == piece
    assert len(bpy.context.scene.objects) == count

print("Blender bridge regression test passed")
