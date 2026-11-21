# OxyBrush Bridge for Blender

Send mesh objects from Blender to OxyBrush, sculpt them, and send them back
([RFC 0010](../../docs/rfcs/0010-blender-bridge.md)).

**Licence:** this folder only is **GPL-3.0-or-later** (see
`oxybrush_bridge/LICENSE`), because Blender add-ons use Blender's Python API.
The rest of OxyBrush is MIT OR Apache-2.0, and nothing in OxyBrush imports
or links this add-on: the two talk through files described by the link
manifest in RFC 0010.

## Use

1. Download `OxyBrush-<version>-blender-extension.zip` from an
   [OxyBrush release](https://github.com/crisprintsstuff/OxyBrush/releases)
   that includes the extension. In Blender 4.2 or newer, open **Edit >
   Preferences > Get Extensions > Install from Disk…** and pick the ZIP.
   If you're building from this checkout, run the following at the repo root
   and install the resulting ZIP.

   ```sh
   blender --command extension build --source-dir integrations/blender/oxybrush_bridge
   ```
2. In the add-on's preferences, set the path to the OxyBrush program
   (`oxybrush`, `oxybrush.exe`, or `OxyBrush.app/Contents/MacOS/oxybrush`).
3. Select mesh objects and choose **Object > Send to OxyBrush** (also in the
   3D view's sidebar, **OxyBrush** tab). OxyBrush opens them, showing
   **Linked to Blender**.
4. In OxyBrush, **File > Send back to Blender** (or click the badge), or
   **Send back to Blender and close**. Blender picks the result up within a
   second: objects with unchanged topology keep their UVs, materials, modifiers,
   and shape-key offsets. Topology changes replace the mesh data on the
   same object. New pieces made in OxyBrush arrive as new objects, and
   repeated sends update those objects. Each update is one
   Blender undo step.

Objects are sent without modifiers applied, in their own space; OxyBrush
places them as they were in the scene.

## Status

The interactive OBJ round trip was verified on Linux with Blender 4.3.2.
A sculpt edit kept the source mesh and its attributes, subdivision replaced
its mesh data, and a new sphere arrived as a separate object. Blender 4.2,
other platforms, extension-directory publication, and glTF exchange remain
pending.

With the extension installed and enabled, run the regression test from
the repository root in a fresh Blender profile.

```sh
blender --background --python-exit-code 1 --python integrations/blender/test_bridge.py
```
