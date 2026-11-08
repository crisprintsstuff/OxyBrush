# SPDX-License-Identifier: GPL-3.0-or-later
# OxyBrush Bridge for Blender: send meshes to OxyBrush and get them back.
# The exchange format (an OxyBrush link manifest plus OBJ files) is
# described in OxyBrush's RFC 0010. Written from Blender's public Python API
# documentation.

import json
import os
import re
import subprocess
import tempfile
import uuid

import bpy
from bpy.props import StringProperty
from bpy.types import AddonPreferences, Operator, Panel

MANIFEST = "bridge.oxylink.json"
POLL_SECONDS = 1.0

# Open links: folder -> {"objects": {sent name: Blender object name}, "seen": mtime}
_links = {}


def _prefs(context):
    return context.preferences.addons[__package__].preferences


def _safe(name):
    s = re.sub(r"[^A-Za-z0-9_-]", "_", name)
    return s or "object"


def _write_obj(path, mesh, name):
    """Vertices in Blender's order (one `v` per vertex), faces as polygons."""
    lines = ["# Blender, for OxyBrush", "o " + name.replace("\n", " ")]
    lines += ["v %r %r %r" % (v.co.x, v.co.y, v.co.z) for v in mesh.vertices]
    lines += ["f " + " ".join(str(i + 1) for i in p.vertices) for p in mesh.polygons]
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    os.replace(tmp, path)


def _read_obj(path):
    verts, faces = [], []
    with open(path, encoding="utf-8") as f:
        for line in f:
            parts = line.split()
            if not parts:
                continue
            if parts[0] == "v":
                verts.append(tuple(float(x) for x in parts[1:4]))
            elif parts[0] == "f":
                faces.append([int(t.split("/")[0]) - 1 for t in parts[1:]])
    return verts, faces


def _matrix_columns(m):
    """Column-major list of 16 numbers (OxyBrush's layout)."""
    return [m[row][col] for col in range(4) for row in range(4)]


class OXYBRUSH_OT_send(Operator):
    """Send the selected mesh objects to OxyBrush to sculpt"""

    bl_idname = "object.oxybrush_send"
    bl_label = "Send to OxyBrush"
    bl_options = {"REGISTER"}

    @classmethod
    def poll(cls, context):
        return any(o.type == "MESH" for o in context.selected_objects)

    def execute(self, context):
        exe = bpy.path.abspath(_prefs(context).executable)
        if not exe or not os.path.isfile(exe):
            self.report({"ERROR"}, "Set the OxyBrush program in the add-on's preferences")
            return {"CANCELLED"}
        if context.object and context.object.mode != "OBJECT":
            bpy.ops.object.mode_set(mode="OBJECT")
        folder = os.path.join(tempfile.gettempdir(), "oxybrush-bridge", uuid.uuid4().hex[:12])
        os.makedirs(os.path.join(folder, "out"), exist_ok=True)
        objects, names, used = [], {}, set()
        for ob in context.selected_objects:
            if ob.type != "MESH":
                continue
            stem = _safe(ob.name)
            while stem in used:
                stem += "_"
            used.add(stem)
            _write_obj(os.path.join(folder, stem + ".obj"), ob.data, ob.name)
            objects.append({
                "name": ob.name,
                "obj": stem + ".obj",
                "result": "out/" + stem + ".obj",
                "matrix": _matrix_columns(ob.matrix_world),
            })
            names[ob.name] = ob.name
        manifest = {"format": "oxybrush-link", "version": 1, "app": "Blender",
                    "objects": objects, "new_pieces": "new"}
        path = os.path.join(folder, MANIFEST)
        with open(path + ".tmp", "w", encoding="utf-8") as f:
            json.dump(manifest, f, indent=1)
        os.replace(path + ".tmp", path)
        try:
            subprocess.Popen([exe, "--link", path])
        except OSError as e:
            self.report({"ERROR"}, "Could not start OxyBrush: %s" % e)
            return {"CANCELLED"}
        _links[folder] = {"objects": names, "seen": 0.0}
        if not bpy.app.timers.is_registered(_poll):
            bpy.app.timers.register(_poll, first_interval=POLL_SECONDS, persistent=True)
        self.report({"INFO"}, "Sent %d object(s) to OxyBrush" % len(objects))
        return {"FINISHED"}


def _apply(folder, link):
    """Apply a `returned.json` OxyBrush wrote. Returns True when the link is closed."""
    with open(os.path.join(folder, "returned.json"), encoding="utf-8") as f:
        returned = json.load(f)
    for item in returned.get("objects", []):
        rel = item.get("file", "")
        path = os.path.normpath(os.path.join(folder, rel))
        if not path.startswith(os.path.normpath(folder) + os.sep) or not os.path.isfile(path):
            continue
        verts, faces = _read_obj(path)
        target = bpy.data.objects.get(link["objects"].get(item.get("name"), ""))
        if item.get("new") or target is None:
            mesh = bpy.data.meshes.new(item.get("name") or "OxyBrush")
            mesh.from_pydata(verts, [], faces)
            mesh.update()
            ob = bpy.data.objects.new(mesh.name, mesh)
            bpy.context.scene.collection.objects.link(ob)
            link["objects"][item.get("name")] = ob.name
            continue
        mesh = target.data
        if item.get("same_topology") and len(verts) == len(mesh.vertices):
            # Same vertices: move them, keeping UVs, materials, groups and shape keys.
            flat = [c for v in verts for c in v]
            mesh.vertices.foreach_set("co", flat)
            mesh.update()
        else:
            new = bpy.data.meshes.new(mesh.name)
            new.from_pydata(verts, [], faces)
            new.update()
            for mat in mesh.materials:
                new.materials.append(mat)
            target.data = new
    bpy.ops.ed.undo_push(message="OxyBrush: Send back")
    return bool(returned.get("closed"))


def _poll():
    for folder in list(_links):
        link = _links[folder]
        path = os.path.join(folder, "returned.json")
        try:
            mtime = os.path.getmtime(path)
        except OSError:
            continue
        if mtime <= link["seen"]:
            continue
        link["seen"] = mtime
        try:
            if _apply(folder, link):
                del _links[folder]
        except (OSError, ValueError) as e:
            print("OxyBrush Bridge:", e)
    return POLL_SECONDS if _links else None


class OXYBRUSH_PT_panel(Panel):
    bl_label = "OxyBrush"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "OxyBrush"

    def draw(self, context):
        self.layout.operator(OXYBRUSH_OT_send.bl_idname, icon="SCULPTMODE_HLT")
        if _links:
            self.layout.label(text="Waiting for OxyBrush (%d link)" % len(_links))


class OxyBrushPreferences(AddonPreferences):
    bl_idname = __package__

    executable: StringProperty(
        name="OxyBrush program",
        description="Path to the OxyBrush executable",
        subtype="FILE_PATH",
    )

    def draw(self, context):
        self.layout.prop(self, "executable")


def _menu(self, context):
    self.layout.operator(OXYBRUSH_OT_send.bl_idname)


CLASSES = (OXYBRUSH_OT_send, OXYBRUSH_PT_panel, OxyBrushPreferences)


def register():
    for c in CLASSES:
        bpy.utils.register_class(c)
    bpy.types.VIEW3D_MT_object.append(_menu)


def unregister():
    bpy.types.VIEW3D_MT_object.remove(_menu)
    if bpy.app.timers.is_registered(_poll):
        bpy.app.timers.unregister(_poll)
    for c in reversed(CLASSES):
        bpy.utils.unregister_class(c)
