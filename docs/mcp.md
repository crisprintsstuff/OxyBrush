# Controlling OxyBrush from an AI agent (MCP)

OxyBrush can speak the [Model Context Protocol](https://modelcontextprotocol.io)
so an agent on **this computer** can sculpt in tandem with you: list pieces,
stroke, remesh, undo, and look at a viewport screenshot. It is **off by
default**. Nothing is sent off the machine. The design is
[RFC 0003](rfcs/0003-mcp.md).

## Turn it on

1. Settings > **Local AI agent**.
2. **Copy connection**. That copies the address (`http://127.0.0.1:<port>/mcp`)
   and a session token.
3. Paste those into your MCP client (below). Use `127.0.0.1`, not a LAN
   address. `localhost` is accepted only when it resolves to loopback; some
   systems try IPv6 first, and this server is IPv4 loopback only.
4. While the agent is talking to OxyBrush, the status bar says **Agent
   connected — Disconnect**. Disconnect (or **New token** in Settings)
   invalidates the token immediately. Unchecking the setting stops the
   listener. A new launch always uses a new token.

The agent and you share one undo history. Agent steps are labeled
`Agent: …`. Don't ask it to open another project over unsaved work unless
you mean it: it has to pass `discard: true`.

Screenshots and automated runs never start the listener.

## Headless, on a file

`oxybrush-cli mcp` speaks MCP on stdin/stdout and does not open a socket.
Give it a project or a mesh, or omit the path to start from a sphere.
Screen-space strokes use an 800×600 virtual view. `screenshot` only works
in the desktop app.

```bash
oxybrush-cli mcp sculpt.oxb
```

The process does not save unless the agent calls `save` or `export`. It
will not overwrite the file it was given except by an explicit `save` of
that project.

## Attach a stdio client to the live app

Clients that can only launch a command still reach the running app:

```bash
oxybrush-cli mcp --attach 127.0.0.1:PORT --token TOKEN
```

`--attach` refuses any host other than `127.0.0.1` or `localhost`. The
port and token are what **Copy connection** shows. The CLI has to be the
one shipped with this version of OxyBrush (`oxybrush-cli` next to the app,
or `cargo run -p oxy-cli -- mcp …`).

## Client setup

Replace `PORT` and `TOKEN` with the values from Settings. The token is a
secret for this session; don't commit it.

### Cursor

`.cursor/mcp.json` in the project, or Cursor Settings > MCP. Either the
running app:

```json
{
  "mcpServers": {
    "oxybrush": {
      "url": "http://127.0.0.1:PORT/mcp",
      "headers": { "Authorization": "Bearer TOKEN" }
    }
  }
}
```

or a stdio launch that attaches to it (works even if the client has no
header field):

```json
{
  "mcpServers": {
    "oxybrush": {
      "command": "oxybrush-cli",
      "args": ["mcp", "--attach", "127.0.0.1:PORT", "--token", "TOKEN"]
    }
  }
}
```

To edit a file with no window at all, drop `--attach` and `--token` and
pass the file path instead: `"args": ["mcp", "/absolute/path/sculpt.oxb"]`.

### Claude Desktop

Claude Desktop launches stdio servers. In
`claude_desktop_config.json` (Settings > Developer > Edit Config):

```json
{
  "mcpServers": {
    "oxybrush": {
      "command": "oxybrush-cli",
      "args": ["mcp", "--attach", "127.0.0.1:PORT", "--token", "TOKEN"]
    }
  }
}
```

Use the full path to `oxybrush-cli` if it is not on `PATH`. Restart Claude
Desktop after saving. For a headless file, use `"args": ["mcp", "C:\\path\\sculpt.oxb"]`
(or the POSIX path).

### Claude Code

```bash
claude mcp add oxybrush -- oxybrush-cli mcp --attach 127.0.0.1:PORT --token TOKEN
```

Or a project `.mcp.json` with the same `command` / `args` shape as Cursor.

### VS Code (Copilot)

`.vscode/mcp.json`:

```json
{
  "servers": {
    "oxybrush": {
      "type": "stdio",
      "command": "oxybrush-cli",
      "args": ["mcp", "--attach", "127.0.0.1:PORT", "--token", "TOKEN"]
    }
  }
}
```

A URL entry (`"type": "http"`, `"url"`, `"headers"`) works too when the
client build speaks Streamable HTTP. OxyBrush's listener accepts POST
`/mcp` with a bearer token and answers JSON. It does not open a
server-sent-events stream; use `--attach` for clients that require one.

## Tools

| Tool | What it does |
|---|---|
| `scene_info` | Document path, modified flag, active piece, brush, symmetry, camera, viewport size, next undo |
| `list_pieces` / `piece_info` / `select_piece` | Pieces by index, id or name |
| `open` | Open an `.oxb` (needs `discard: true` if the document is dirty) or import a mesh |
| `save` | Atomic project save. Path optional once the document has one |
| `import` | Add meshes as pieces (one undo step) |
| `export` | glTF/GLB, OBJ, STL or PLY. Refuses to overwrite the project or an opened input |
| `add_primitive` | `sphere`, `cube` or `plane` |
| `select_brush` | Any built-in brush, by name or key (`draw`, `clay_strips`, …) |
| `set_brush` | `size` (same units as the size slider) and `strength` (0–1) |
| `set_symmetry` | Mirror `x`/`y`/`z`, and `radial` copies around `radial_axis` |
| `stroke` | `space` `world` or `screen`, `points`, optional `invert` and `pressure` |
| `mask` | `clear`, `fill`, `invert`, `blur`, `sharpen`, `grow`, `shrink`, `cavities`, `peaks` |
| `remesh` | Voxel remesh; optional `resolution` 8–320 |
| `decimate` | `keep_percent` or `triangles` |
| `subdivide` | Optional `smooth` (default true) |
| `undo` / `redo` | The shared history, including your own steps |
| `set_camera` | `view` (`front`, `back`, `left`, `right`, `top`, `bottom`), angles, or `frame: true` |
| `screenshot` | PNG of the viewport in the desktop app. Screen points match its pixels |

`stroke` points are `[x, y]` in viewport pixels (top-left origin) or
`[x, y, z]` in world units. Take a screenshot first when working in the
app: the image is the viewport, and a point in the image is a screen point.

## What the agent cannot do

- Reach any host but this machine's loopback, or any port you did not opt into.
- See the token from inside a tool. You paste it into the client yourself.
- Bypass undo. If a stroke was wrong, `undo` (or Ctrl+Z) removes it.
