# RFC 0011: Plugin API, phase 2: in-app WebAssembly plugins

- **Status:** proposed for the maintainer; first version implemented in the same change for review
- **Follows:** [RFC 0007](0007-plugin-api-read-access.md) (read access over MCP),
  which named WebAssembly as the in-process route and listed panels,
  commands and overlays as phase 2.

## Problem

Plugins today are Python scripts talking to OxyBrush over the local MCP
server ([RFC 0003](0003-mcp.md)). That suits automation and external tools,
but:

- users must have Python installed and switch the agent server on;
- a plugin can't add a panel or a command to the app, or draw in the
  viewport;
- every call crosses a process boundary as JSON, too slow for anything per
  frame.

## Decision

OxyBrush runs **WebAssembly plugins inside the app**, sandboxed, with the
pure-Rust interpreter [wasmi](https://github.com/wasmi-labs/wasmi)
(MIT/Apache-2.0; no C toolchain, in keeping with the pure-Rust rule).

### A plugin

A folder in the plugins folder holding:

- `plugin.json`: name, version, description, `"api": 1`, the
  **capabilities** it needs, its **commands** (buttons) and **params**
  (sliders, with min, max and default);
- `plugin.wasm`, or `plugin.wat` (WebAssembly text, handy for small
  plugins and examples). Any language that compiles to WebAssembly works.

OxyBrush ships plugins written in WebAssembly text in
`crates/oxy-plugin/plugins/` (listed in `oxy_plugin::BUNDLED`) and installs
any that are missing into the plugins folder: **Squash**, **Displace**,
**Mask by Height**, **Color Ramp**, **Array** and **Measure** (later
releases add more; the [plugin guide](../plugins/README.md) lists the
current set). They are
useful tools and examples for plugin authors at the same time.

### What a plugin can do

| Capability | Gives |
|---|---|
| *(none)* | `log`, `param`, `piece_count`, `active_piece` |
| `read_mesh` | vertex and triangle counts, positions, triangles, mask of any piece |
| `edit_mesh` | `write_positions`: new positions for every vertex of a piece |
| `read_scene` | `scene_state`: the RFC 0007 scene JSON (pieces, versions, camera) |
| `overlay` | `draw_line` in world space, from its `overlay()` export, each frame its panel is open |

The full host API (signatures, error codes, memory layout) is documented
at the top of `crates/oxy-plugin/src/lib.rs`. It uses only numbers and
offsets into the plugin's memory, so it needs no bindings generator, and
it is versioned (`"api"`): a plugin built for another version is refused
with a message, not run.

### In the app

- The **Plugins** menu in the menu bar lists every plugin: a submenu per
  in-app plugin with its commands and its panel, the Python scripts, and
  Install add-on, Open plugins folder, Reload and Manage.
- **Settings > Plugins** lists in-app plugins with what each can do, and
  any that failed to load and why. **Open panel** shows a plugin's
  window: its sliders and command buttons.
- **A command runs on a worker thread** (architecture invariant 5). Pieces it
  may edit are locked while it runs. Its edits come back as data and are
  applied as one undo step per piece, labelled `Plugin: <name> — <command>`
  (invariant 4), just as agent edits are labelled `Agent:`.
- **Overlays** are drawn over the viewport while the plugin's panel is
  open.

### Sandbox

- **No access to anything but the host API:** no files, network, clock,
  environment or WASI. A module importing anything else doesn't load. This
  keeps the "no network access" rule intact.
- **Capabilities are checked on every call.** An undeclared one returns
  an error code. Manifests with unknown capabilities are refused, so a
  plugin never runs with more, or less, than it asked for.
- **Fuel:** a command gets about a billion instructions and an overlay
  frame twenty million. A plugin that runs past that is stopped with a
  message and stays usable.
- **Memory** is capped at 512 MB. Pointers are bounds-checked, and
  non-finite positions are refused.
- **The plugin never holds a reference into the document.** It gets a
  snapshot and returns edits, so a crash or a bad write can't corrupt the
  scene, and Undo always works.

wasmi is used with its portable (loop-based) interpreter, which can't
overflow the native stack in any build profile.

## API 2

Additive, so `"api": 1` plugins keep working (OxyBrush runs API 1 and 2):

- `read_normals` and `read_colors` (`read_mesh`);
- `write_mask` and `write_colors` (`edit_mesh`), whole-piece like
  `write_positions`; a command's edits to one piece (positions, mask,
  colors) are applied together as one undo step;
- `add_piece` with a new capability, `add_pieces`: a command's new pieces
  are added as one more undo step, and the last becomes active;
- every plugin command is listed in the command palette as
  `Plugin: <name> — <command>`.

## Add-on packages (`.oxyaddon`)

One file to share and install a plugin, of either kind:

- An `.oxyaddon` is a ZIP archive of the plugin folder, with `plugin.json`
  at the top. `"kind"` is `"wasm"` (default: `plugin.wasm` or
  `plugin.wat`, run in the sandbox) or `"python"` (`"entry"` names the
  script, default `main.py`; it runs as a separate process over the local
  agent server, like any Python plugin, so Python plugins keep working).
  An optional `"id"` names the folder it installs to.
- **Install:** Settings > Plugins > **Install add-on…**, or open the file
  with OxyBrush. It unpacks into `<plugins>/<id>/`, replacing an older
  version; the plugin then shows in the in-app or Python list.
- **Make one:** `oxybrush-cli addon pack <folder> <out.oxyaddon>` (checks
  it as an install would); `oxybrush-cli addon inspect <file>` lists it.
- **Safety:** entry paths must stay inside the add-on folder (no absolute
  paths, no `..`); at most 512 files and 256 MB unpacked; CRCs checked;
  only stored and deflate entries, no encryption. Files are written to a
  temporary folder and renamed into place, so a failed install changes
  nothing. A WebAssembly add-on is loaded once before installing, so a
  broken one is refused with its error. Installing does not run anything;
  Python add-ons run only when you click Run, as before.
- **Remove** (Settings > Plugins, confirmed with a second click) deletes a
  plugin you installed: its folder, or a loose Python script. Only entries
  directly inside the plugins folder qualify, and not the built-in ones.

## Not in this version

- More edit kinds: face groups and topology changes of existing pieces.
  (Mask, vertex color and adding pieces arrived in API 2, below; face
  groups, hidden faces and topology in API 3,
  [RFC 0013](0013-plugin-api-3.md).)
- Brushes written as plugins (a per-dab callback): that is on the per-dab
  path and needs measuring first (`dab_timing`).
- Command shortcuts for plugin commands. (They are in the command palette
  since API 2.)
- A Rust SDK crate for plugin authors. It needs `unsafe` FFI declarations
  for the imports, which this workspace forbids, so it belongs in its own
  repository.
- Text, images and widgets other than sliders and buttons in panels.
  (Toggles, choices, colors and text arrived in API 3; images haven't.)

## Verification

- `oxy-plugin` tests:
  - manifest parsing and rejection;
  - the Squash example's edit, mask and overlay;
  - every capability denied when undeclared;
  - bad writes refused (count, piece, memory bounds);
  - a runaway loop stopped and the plugin usable after;
  - memory cap;
  - modules missing `run` or `memory`, or importing WASI, refused;
  - discovery with failures reported.
- Editor test: the Squash example runs on a worker thread, locks the piece,
  halves its height, is one `Plugin: Squash — Squash` undo step, undoes
  exactly, and draws its overlay.
- In the app (Linux, Xvfb): the Squash panel and the Plugins settings tab,
  with a run's status message.
