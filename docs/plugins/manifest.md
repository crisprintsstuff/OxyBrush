# Plugin manifest reference (`plugin.json`)

Every in-app plugin has a `plugin.json` next to its module. OxyBrush checks
it when it loads the plugin. A manifest it doesn't understand is refused,
with the reason shown in **Settings > Plugins**, rather than run with less
or more than it asked for.

```json
{
  "name": "Group by Height",
  "version": "1.0",
  "description": "Splits the active piece into bands of face groups along an axis.",
  "api": 3,
  "capabilities": ["read_mesh", "edit_mesh"],
  "commands": [
    { "id": "group", "label": "Group in bands", "preview": true }
  ],
  "params": [
    { "id": "axis", "label": "Along", "type": "choice", "options": ["X", "Y", "Z"], "default": 1 },
    { "id": "bands", "label": "Bands", "min": 2, "max": 32, "step": 1, "default": 4, "help": "How many face groups to make" },
    { "id": "note", "type": "label", "text": "Each band becomes a face group." }
  ]
}
```

## Top level

| Field | Required | |
|---|---|---|
| `name` | yes | Shown in the Plugins menu, panel title and undo history |
| `api` | yes | The host API version it was written for: 1 to 4 |
| `version` | no | Your plugin's own version (for you and people sharing it) |
| `description` | no | One or two sentences, shown in menus and the panel |
| `capabilities` | no | What it may do (below); none by default |
| `commands` | no | Its buttons (below) |
| `params` | no | Its panel settings (below) |

## Capabilities

| Capability | Allows | API |
|---|---|---|
| *(none)* | `log`, `param`, `param_color`, `progress`, `piece_count`, `active_piece` | 1 (3 for `param_color`, `progress`) |
| `read_mesh` | reading any piece: counts, positions, triangles, mask, normals, colors, face groups, hidden faces | 1 (more in 2 and 3) |
| `edit_mesh` | writing a piece's positions, mask, colors, face groups and hidden faces | 1 (more in 2 and 3) |
| `edit_topology` | replacing a piece's vertices and triangles (`write_mesh`); this can discard detail and subdivision levels, so it's separate | 3 |
| `add_pieces` | creating new pieces | 2 |
| `read_scene` | the scene as JSON: pieces, versions, camera; the active piece's living strokes (`strokes_state`, 4) | 1 |
| `edit_strokes` | Strokes window actions on living strokes: strength, groups, delete, bake (`stroke_command`) | 4 |
| `overlay` | drawing lines over the viewport from `overlay()` | 1 |

Settings > Plugins shows them in words ("reads meshes", "changes
topology", …) so people know what they're installing.

## Commands

```json
{ "id": "inflate", "label": "Inflate", "preview": true }
```

| Field | | |
|---|---|---|
| `id` | required | A stable identifier |
| `label` | optional | The button text; defaults to `id` |
| `preview` | optional, API 3 | `true`: clicking it shows the result live in place of the active piece, re-run as settings change, and applied with **Accept** |

`run(command)` is called with the command's index in this list, starting
at 0.

A preview shows only the command's edits to the **active piece**. New
pieces, and edits to other pieces, are applied on Accept.

## Params

`param(index)` reads a param by its index in this list. Every param has an
`id` (required, and the key its remembered value is saved under) and a
`label`. With API 3 it can also have a `type` and a `help` tooltip.

| `type` | Shows | Fields | `param(index)` gives |
|---|---|---|---|
| `"slider"` (default) | a slider | `min` (default 0), `max` (default 1), `default` (default `min`), `step` (API 3; whole numbers if it is 1) | the value |
| `"toggle"` | a checkbox | `default`: `true` or `false` | 1 or 0 |
| `"choice"` | a drop-down | `options`: a list of names; `default`: an index | the index chosen |
| `"color"` | a color button | `default`: `[r, g, b]` in 0..=1 (sRGB) | 0; read it with `param_color(index, ptr)` |
| `"label"` | text in the panel | `text` | 0 |

Rules:
- A slider needs `min < max`, and a `step` above 0.
- A choice needs at least one option.
- A color's default needs three numbers.
- An unknown `type` is refused.
- Values are kept in range: within `min`..`max`, on a `step`, 0 or 1, or a
  valid option index.

Remembered values are restored when the plugin loads. They're keyed by the
plugin's folder name and the param's `id`, so renaming either starts from
the defaults. A remembered value that no longer fits (a smaller range,
fewer options) is clamped, or dropped if it can't be.

## Files

- `plugin.wasm` (a compiled module) or `plugin.wat` (WebAssembly text) next
  to `plugin.json`. If both are there, `plugin.wasm` is used.
- In an `.oxyaddon`, `"kind": "wasm"` (the default) marks an in-app plugin.
  `"kind": "python"` with `"entry"` marks a Python script. An optional
  `"id"` names the folder it installs to.
