"""
Client communicating with OxyBrush over the Model Context Protocol (JSON-RPC 2.0).
"""

import json
import os
import re
import urllib.request
import urllib.error
from typing import Optional, List, Dict, Any, Union


class OxyBrushError(Exception):
    """Raised when an OxyBrush tool call fails or the server cannot be reached."""
    pass


class Session:
    """An active session with a running OxyBrush instance."""

    def __init__(self, endpoint: Optional[str] = None, token: Optional[str] = None):
        self.endpoint = endpoint or os.environ.get("OXYBRUSH_MCP_ENDPOINT", "http://127.0.0.1:41799/mcp")
        self.token = token or os.environ.get("OXYBRUSH_MCP_TOKEN", "")
        self._request_id = 1
        self._initialized = False

    def _send_raw(
        self, method: str, params: Optional[Dict[str, Any]] = None, is_notification: bool = False
    ) -> Optional[Dict[str, Any]]:
        req_body: Dict[str, Any] = {
            "jsonrpc": "2.0",
            "method": method,
        }
        if not is_notification:
            req_body["id"] = self._request_id
            self._request_id += 1
        if params is not None:
            req_body["params"] = params

        data = json.dumps(req_body).encode("utf-8")
        headers = {"Content-Type": "application/json"}
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"

        req = urllib.request.Request(self.endpoint, data=data, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                if is_notification:
                    return None
                resp_bytes = resp.read()
                if not resp_bytes:
                    return None
                resp_data = json.loads(resp_bytes.decode("utf-8"))
                if "error" in resp_data:
                    err = resp_data["error"]
                    msg = err.get("message", str(err)) if isinstance(err, dict) else str(err)
                    raise OxyBrushError(msg)
                return resp_data.get("result", {})
        except urllib.error.HTTPError as e:
            err_msg = e.read().decode("utf-8", errors="replace")
            raise OxyBrushError(f"HTTP {e.code}: {err_msg}") from e
        except urllib.error.URLError as e:
            raise OxyBrushError(f"Failed to connect to OxyBrush at {self.endpoint}: {e.reason}") from e

    def _ensure_initialized(self):
        if not self._initialized:
            self._send_raw(
                "initialize",
                {
                    "protocolVersion": "2024-11-05",
                    "clientInfo": {"name": "oxybrush-python", "version": "0.1.0"},
                },
            )
            self._send_raw("notifications/initialized", is_notification=True)
            self._initialized = True

    def call_tool(self, _tool_name: str, **kwargs) -> Dict[str, Any]:
        """Call a tool on the OxyBrush instance."""
        self._ensure_initialized()
        result = self._send_raw("tools/call", {"name": _tool_name, "arguments": kwargs}) or {}
        if result.get("isError"):
            content = result.get("content", [])
            msg = content[0].get("text") if content else "Tool error"
            raise OxyBrushError(msg)
        return result

    def list_tools(self) -> List[Dict[str, Any]]:
        """List all available tools from OxyBrush."""
        self._ensure_initialized()
        res = self._send_raw("tools/list") or {}
        return res.get("tools", [])

    def scene_info(self) -> str:
        """Return information about the current scene and camera."""
        res = self.call_tool("scene_info")
        return res["content"][0]["text"]

    def list_pieces(self) -> List[Dict[str, Any]]:
        """List all pieces in the document."""
        res = self.call_tool("list_pieces")
        text = res["content"][0]["text"]
        pieces = []
        for line in text.splitlines():
            line = line.strip()
            if not line:
                continue
            m = re.match(
                r"^(\d+):\s+(.+?)\s+id\s+(\d+)\s+(active|idle)(\s+hidden)?\s+(\d+)\s+triangles,\s+(\d+)\s+vertices",
                line,
            )
            if m:
                idx, name, piece_id, status, hidden, tris, verts = m.groups()
                pieces.append({
                    "index": int(idx),
                    "name": name,
                    "id": int(piece_id),
                    "is_active": status == "active",
                    "visible": not bool(hidden),
                    "triangles": int(tris),
                    "vertices": int(verts),
                    "raw": line,
                })
            else:
                pieces.append({"raw": line})
        return pieces

    def piece_info(
        self, id: Optional[int] = None, index: Optional[int] = None, name: Optional[str] = None
    ) -> str:
        """Return detailed statistics for a piece."""
        args: Dict[str, Any] = {}
        if id is not None:
            args["id"] = id
        if index is not None:
            args["index"] = index
        if name is not None:
            args["name"] = name
        res = self.call_tool("piece_info", **args)
        return res["content"][0]["text"]

    def select_piece(
        self, id: Optional[int] = None, index: Optional[int] = None, name: Optional[str] = None
    ) -> str:
        """Select the active piece."""
        args: Dict[str, Any] = {}
        if id is not None:
            args["id"] = id
        if index is not None:
            args["index"] = index
        if name is not None:
            args["name"] = name
        res = self.call_tool("select_piece", **args)
        return res["content"][0]["text"]

    def add_primitive(self, kind: str = "cube") -> str:
        """Add a primitive piece to the scene ('cube', 'sphere', 'plane')."""
        res = self.call_tool("add_primitive", kind=kind)
        return res["content"][0]["text"]

    def select_brush(self, name: str) -> str:
        """Select active brush by name ('Draw', 'Clay', 'Smooth', 'Grab', etc.)."""
        res = self.call_tool("select_brush", name=name)
        return res["content"][0]["text"]

    def set_brush(
        self, size: Optional[float] = None, strength: Optional[float] = None
    ) -> str:
        """Set brush radius in points and/or strength (0..=1)."""
        args: Dict[str, Any] = {}
        if size is not None:
            args["size"] = size
        if strength is not None:
            args["strength"] = strength
        res = self.call_tool("set_brush", **args)
        return res["content"][0]["text"]

    def set_symmetry(
        self,
        x: Optional[bool] = None,
        y: Optional[bool] = None,
        z: Optional[bool] = None,
        radial: Optional[int] = None,
        axis: Optional[str] = None,
    ) -> str:
        """Configure mirror planes and radial symmetry."""
        args: Dict[str, Any] = {}
        if x is not None:
            args["x"] = x
        if y is not None:
            args["y"] = y
        if z is not None:
            args["z"] = z
        if radial is not None:
            args["radial"] = radial
        if axis is not None:
            args["axis"] = axis
        res = self.call_tool("set_symmetry", **args)
        return res["content"][0]["text"]

    def stroke(
        self,
        points: List[Union[List[float], Dict[str, float]]],
        space: str = "world",
        invert: bool = False,
        pressure: float = 1.0,
    ) -> str:
        """Apply a stroke through a sequence of points in 'world' or 'screen' space."""
        clean_points = []
        for p in points:
            if isinstance(p, dict):
                clean_points.append([p.get("x", 0.0), p.get("y", 0.0), p.get("z", 0.0)])
            else:
                clean_points.append(list(p))
        res = self.call_tool("stroke", points=clean_points, space=space, invert=invert, pressure=pressure)
        return res["content"][0]["text"]

    def mask(self, op: str) -> str:
        """Mask operation: 'clear', 'invert', 'blur', 'sharpen', 'grow', 'shrink'."""
        res = self.call_tool("mask", op=op)
        return res["content"][0]["text"]

    def remesh(self, resolution: Optional[int] = None) -> str:
        """Voxel remesh the active piece."""
        args = {"resolution": resolution} if resolution is not None else {}
        res = self.call_tool("remesh", **args)
        return res["content"][0]["text"]

    def decimate(
        self, keep_percent: Optional[float] = None, triangles: Optional[int] = None
    ) -> str:
        """Simplify geometry to a percentage or target triangle count."""
        args: Dict[str, Any] = {}
        if keep_percent is not None:
            args["keep_percent"] = keep_percent
        if triangles is not None:
            args["triangles"] = triangles
        res = self.call_tool("decimate", **args)
        return res["content"][0]["text"]

    def subdivide(self, smooth: bool = True) -> str:
        """Subdivide the active piece."""
        res = self.call_tool("subdivide", smooth=smooth)
        return res["content"][0]["text"]

    def undo(self) -> str:
        """Undo the last edit."""
        res = self.call_tool("undo")
        return res["content"][0]["text"]

    def redo(self) -> str:
        """Redo the last undone edit."""
        res = self.call_tool("redo")
        return res["content"][0]["text"]

    def open(self, path: str, discard: bool = False) -> str:
        """Open a .oxb project file."""
        res = self.call_tool("open", path=path, discard=discard)
        return res["content"][0]["text"]

    def save(self, path: Optional[str] = None) -> str:
        """Save the document to .oxb."""
        args = {"path": path} if path else {}
        res = self.call_tool("save", **args)
        return res["content"][0]["text"]

    def import_file(self, path: str) -> str:
        """Import an OBJ or STL mesh into the scene as new pieces."""
        res = self.call_tool("import", path=path)
        return res["content"][0]["text"]

    def export_file(self, path: str) -> str:
        """Export visible pieces to glTF (.glb), OBJ, STL or PLY."""
        res = self.call_tool("export", path=path)
        return res["content"][0]["text"]

    def screenshot(self) -> bytes:
        """Capture the current viewport as PNG bytes."""
        import base64
        res = self.call_tool("screenshot")
        for item in res.get("content", []):
            if item.get("type") == "image":
                return base64.b64decode(item["data"])
        raise OxyBrushError("No image returned from screenshot")


def connect(endpoint: Optional[str] = None, token: Optional[str] = None) -> Session:
    """Connect to a running OxyBrush instance."""
    return Session(endpoint=endpoint, token=token)


def attach() -> Session:
    """Attach to the parent OxyBrush session running this plugin script."""
    return connect()
