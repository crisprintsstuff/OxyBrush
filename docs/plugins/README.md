# In-app plugins (WebAssembly)

In-app plugins add commands and panels to OxyBrush. They run **inside the
app, in a sandbox**: a plugin can only do what its manifest declares, it
can't touch files or the network, and every change it makes is one normal
undo step.

A plugin can:
- read and change the meshes: positions, mask, vertex color, face groups,
  hidden faces, and, since API 3, the triangles themselves;
- add new pieces;
- draw lines over the viewport;
- show sliders, switches, choices and colors in its own panel;
- show its result live before you apply it (API 3).

You can write one in any language that compiles to WebAssembly (Rust, C,
Zig, AssemblyScript and others), or directly in WebAssembly text, the way
the plugins that ship with OxyBrush are written.

Also see:
- [Manifest reference](manifest.md): everything `plugin.json` can say.
- [Host API reference](api-reference.md): every function a plugin can
  call.

Python scripts are a different kind of plugin. They run as a separate
program and talk to OxyBrush over MCP, like a local AI agent, but on a
connection of their own: each run gets its own loopback listener and
token, closed when the script ends. Running one doesn't turn on
Settings > Local AI agent; the status bar shows "Plugin '*name*' running",
and its edits are undo steps labelled `Plugin: <name>: …` rather than
`Agent: …`. The script finds the connection in `OXYBRUSH_MCP_ENDPOINT` and
`OXYBRUSH_MCP_TOKEN` (the Python SDK's `oxybrush.attach()` reads them).

## Using plugins

- **Plugins** in the menu bar lists every in-app plugin. Each one has a
  submenu with its commands and its **panel**, where you set it up.
- **Settings > Plugins** shows what each plugin is allowed to do ("reads
  meshes", "changes topology", …). It also lists any plugin that failed to
  load, and why, and has **Remove** for plugins you installed.
- **Install add-on…** installs an `.oxyaddon` file: a plugin packed into
  one file. **Open plugins folder** shows where plugins live, and **Reload
  plugins** picks up changes.
- **The command palette** lists every plugin command as
  `Plugin: <name> — <command>`.

### Live preview (API 3)

Some commands show their result before applying it: Displace and Group by
Height, for example. Clicking one opens the plugin's panel and shows the
result in place of the active piece. While it's shown:
- moving a setting updates it;
- **Accept** applies exactly what you see, as one undo step;
- **Cancel**, closing the panel, or editing the piece some other way
  leaves the piece as it was.

### Long commands

While a command runs, its panel shows a progress bar (if the plugin
reports progress) and **Stop**. Nothing a stopped command did is applied.
A plugin that runs far too long is stopped automatically.

### Settings are remembered

A plugin's panel settings are saved with OxyBrush's settings and come back
next time. **Reset** in the panel puts them back to the plugin's defaults.

## The plugins folder

A plugin is a folder inside the plugins folder:

```
plugins/
  my_plugin/
    plugin.json     the manifest: name, commands, settings, capabilities
    plugin.wasm     the compiled module (or plugin.wat, WebAssembly text)
```

**Plugins > Open plugins folder** opens it. It is:

| System | Folder |
|---|---|
| Windows | `%LOCALAPPDATA%\OxyBrush\plugins` |
| macOS | `~/Library/Application Support/oxybrush/plugins` |
| Linux | `~/.local/share/oxybrush/plugins` |

The Flatpak keeps it inside its own sandbox; use **Open plugins folder**
to find it.

The plugins that ship with OxyBrush are copied into this folder when it
starts, if they aren't already there:
- Squash
- Displace
- Mask by Height
- Color Ramp
- Array
- Measure
- Group by Height
- Flip Faces
- Group by Angle
- Hide Faces by Angle
- Fill Holes
- Simplify Mesh
- Color by Group

Group by Angle, Hide Faces by Angle and Color by Group show a result
before you accept it. Fill Holes and Simplify Mesh change the triangles
and can be stopped; they are not previews, because they can take a
moment. Group by Angle reads and writes face groups from the angle
between neighbouring faces. Hide Faces by Angle is the one that writes
hidden faces. Color by Group is the one with a color setting. Fill Holes
and Simplify Mesh replace the triangles (`edit_topology`) and write the
mask, color, groups and hidden faces back afterwards, since a new mesh
starts blank.

Their source (`plugin.wat`) is a good place to start. A plugin you've
changed is never overwritten. To get a newer version of a bundled plugin,
delete its folder and restart.

## Your first plugin, in WebAssembly text

This plugin logs a message in the status bar. Make a folder `hello` in
the plugins folder with these two files.

`plugin.json`:

```json
{
  "name": "Hello",
  "version": "1.0",
  "description": "Says hello in the status bar.",
  "api": 3,
  "commands": [{ "id": "hello", "label": "Say hello" }]
}
```

`plugin.wat`:

```wat
(module
  (import "oxybrush" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "Hello from a plugin")
  (func (export "run") (param $command i32)
    (call $log (i32.const 0) (i32.const 19))))
```

Then choose **Plugins > Reload plugins**, and **Plugins > Hello > Say
hello**.

Every plugin:
- exports its `memory`;
- exports `run(command: i32)`, which is called with the index of the
  command clicked (0 for the first in `commands`);
- may export `overlay()`, called each frame while its panel is open, to
  draw lines (needs the `overlay` capability).

## A plugin in Rust

Any language works if it compiles to `wasm32-unknown-unknown` with no
WASI. This Rust plugin moves the active piece up, except where it is
masked, with a live preview.

`Cargo.toml`:

```toml
[package]
name = "lift"
version = "1.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = "s"
lto = true
```

`src/lib.rs`:

```rust
// The host functions this plugin uses, from the "oxybrush" import module.
#[link(wasm_import_module = "oxybrush")]
unsafe extern "C" {
    fn log(ptr: i32, len: i32);
    fn param(index: i32) -> f32;
    fn active_piece() -> i32;
    fn vertex_count(piece: i32) -> i32;
    fn read_positions(piece: i32, ptr: i32, max: i32) -> i32;
    fn read_mask(piece: i32, ptr: i32, max: i32) -> i32;
    fn write_positions(piece: i32, ptr: i32, count: i32) -> i32;
}

fn say(text: &str) {
    unsafe { log(text.as_ptr() as i32, text.len() as i32) }
}

/// Called with the index of the command the user clicked.
#[unsafe(no_mangle)]
pub extern "C" fn run(_command: i32) {
    unsafe {
        let piece = active_piece();
        let n = vertex_count(piece);
        if n <= 0 {
            return;
        }
        let mut pos = vec![0f32; n as usize * 3];
        let mut mask = vec![0f32; n as usize];
        read_positions(piece, pos.as_mut_ptr() as i32, n);
        read_mask(piece, mask.as_mut_ptr() as i32, n);
        // The piece's height, from its lowest and highest vertex.
        let ys = pos.iter().skip(1).step_by(3);
        let (lo, hi) = ys.fold((f32::MAX, f32::MIN), |(lo, hi), &y| (lo.min(y), hi.max(y)));
        let lift = (hi - lo) * param(0) / 100.0;
        for (i, m) in mask.iter().enumerate() {
            pos[i * 3 + 1] += lift * (1.0 - m);
        }
        if write_positions(piece, pos.as_ptr() as i32, n) == 0 {
            say("Lifted");
        }
    }
}
```

`plugin.json`:

```json
{
  "name": "Lift",
  "version": "1.0",
  "description": "Moves the active piece up, except where it is masked.",
  "api": 3,
  "capabilities": ["read_mesh", "edit_mesh"],
  "commands": [{ "id": "lift", "label": "Lift", "preview": true }],
  "params": [{ "id": "amount", "label": "Amount (% of height)", "min": 0, "max": 50, "default": 10 }]
}
```

Build it, and put the module next to the manifest as `plugin.wasm`:

```bash
rustup target add wasm32-unknown-unknown
```

```bash
cargo build --release --target wasm32-unknown-unknown
```

```bash
cp target/wasm32-unknown-unknown/release/lift.wasm plugin.wasm
```

Pointers are plain `i32` offsets into the plugin's own memory, so a
`Vec`'s pointer can be passed straight to the host.

## Sharing a plugin: `.oxyaddon`

An add-on is a ZIP of the plugin folder, with `plugin.json` at the top.
`oxybrush-cli` (in the portable downloads) makes and checks one:

```bash
oxybrush-cli addon pack my_plugin my_plugin.oxyaddon
```

```bash
oxybrush-cli addon inspect my_plugin.oxyaddon
```

To install one, use **Plugins > Install add-on…**, or open the file with
OxyBrush. Installing never runs the plugin. A broken plugin is refused, with
the reason, before anything is installed.

## The sandbox

- **Only the host API.** There are no files, no network, no clock, no
  environment and no WASI. A module that imports anything else doesn't
  load.
- **Only what's declared.** Each function checks the plugin's capabilities
  and returns an error code if it wasn't declared. A manifest with an
  unknown capability doesn't load, so a plugin never gets more than it
  asked for.
- **Time and memory are limited.** A command gets about a billion
  instructions and an overlay frame about twenty million; a plugin that
  runs past that is stopped and stays usable. Memory is capped at 512 MB.
- **Edits are data.** A plugin never holds the document. It gets a copy of
  the scene and returns the changes it wants, which OxyBrush checks and
  applies as one undo step per piece, labelled `Plugin: <name> — <command>`.
  A plugin that crashes can't damage your work.

## Tips

- **Handle error codes.** Every function that can fail returns a negative
  code; see the [host API reference](api-reference.md#error-codes).
- **Masks.** `read_mask` gives 1 for fully protected vertices. Most
  plugins should leave those where they are.
- **Large meshes.** A piece's data is copied into your memory, about 12
  bytes per vertex per array. Grow memory before reading (`memory.grow`,
  or let your language's allocator do it). Give up with a message if it
  can't grow.
- **Progress.** For anything that takes more than a moment, call
  `progress` every few thousand items, and return early when it returns 1.
- **Preview commands should be fast.** They run on every setting change.
  Heavy commands are better without `"preview"`.
- **Debugging.** `log` writes to the status bar and the log file
  (**Help > Open log folder**). A plugin that fails to load says why in
  **Settings > Plugins**.

## API versions

`"api"` in the manifest says which version a plugin was written for.
OxyBrush runs every version from 1 to the current one, so old plugins keep
working. A plugin that needs a newer version than the app has is refused
with a message.

| API | Since | Adds |
|---|---|---|
| 1 | 3.0.2-beta | read positions, triangles and mask; write positions; scene JSON; overlay lines; sliders |
| 2 | 3.0.3-beta | normals and colors; write mask and colors; add pieces; commands in the palette |
| 3 | 3.0.7-beta | face groups and hidden faces; replace a piece's mesh (`edit_topology`); live preview; toggles, choices, colors, labels and step sliders; `param_color`; progress and Stop; remembered settings |
| 4 | 4.0.0-beta.10 | living strokes: read the list (`strokes_state`) and run Strokes window actions (`stroke_command`, capability `edit_strokes`) |
