"""
Client communicating with OxyBrush over the Model Context Protocol (JSON-RPC 2.0).
"""

import json
import os
import re
import urllib.request
import urllib.error
from dataclasses import dataclass
from typing import Optional, List, Dict, Any, Union, Tuple


@dataclass
class MeshStats:
    """Aggregated mesh metrics for the active piece or scene."""
    vertex_count: int
    triangle_count: int
    piece_count: int
    subdivision_level: int = 0
    bounding_box_min: Tuple[float, float, float] = (-1.0, -1.0, -1.0)
    bounding_box_max: Tuple[float, float, float] = (1.0, 1.0, 1.0)


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
                r"^(\d+):\s+(.+?)\s+id\s+(\d+)\s+(active|idle)(\s+hidden)?\s+(\d+)\s+triangles,\s+(\d+)\s+vertices(?:,\s+mask\s+\d+)?(?:,\s+bounds\s+\[([^\]]+)\]\s+\.\.\s+\[([^\]]+)\])?",
                line,
            )
            if m:
                idx, name, piece_id, status, hidden, tris, verts, b_min_str, b_max_str = m.groups()
                b_min = (-1.0, -1.0, -1.0)
                b_max = (1.0, 1.0, 1.0)
                if b_min_str and b_max_str:
                    try:
                        b_min = tuple(float(x.strip()) for x in b_min_str.split(","))
                        b_max = tuple(float(x.strip()) for x in b_max_str.split(","))
                    except ValueError:
                        pass
                pieces.append({
                    "index": int(idx),
                    "name": name,
                    "id": int(piece_id),
                    "is_active": status == "active",
                    "visible": not bool(hidden),
                    "triangles": int(tris),
                    "vertices": int(verts),
                    "bounds_min": b_min,
                    "bounds_max": b_max,
                    "raw": line,
                })
            else:
                pieces.append({"raw": line})
        return pieces

    def get_mesh_stats(self) -> MeshStats:
        """Return aggregated mesh statistics for the active piece or scene."""
        pieces = self.list_pieces()
        active = next((p for p in pieces if p.get("is_active")), pieces[0] if pieces else {})
        v_total = sum(p.get("vertices", 0) for p in pieces)
        t_total = sum(p.get("triangles", 0) for p in pieces)
        b_min = active.get("bounds_min", (-1.0, -1.0, -1.0))
        b_max = active.get("bounds_max", (1.0, 1.0, 1.0))
        return MeshStats(
            vertex_count=active.get("vertices", v_total),
            triangle_count=active.get("triangles", t_total),
            piece_count=len(pieces),
            subdivision_level=0,
            bounding_box_min=b_min,
            bounding_box_max=b_max,
        )

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

    def add_primitive(
        self,
        kind: Optional[str] = None,
        primitive_type: Optional[str] = None,
        position: Optional[Any] = None,
        radius: Optional[float] = None,
    ) -> str:
        """Add a primitive piece to the scene ('cube', 'sphere', 'plane')."""
        chosen = kind or primitive_type or "sphere"
        res = self.call_tool("add_primitive", kind=chosen)
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

    def apply_stroke(
        self,
        points: List[Union[List[float], Dict[str, float]]],
        brush: Optional[str] = None,
        radius: Optional[float] = None,
        strength: Optional[float] = None,
        space: str = "world",
        invert: bool = False,
        pressure: float = 1.0,
    ) -> str:
        """Configure brush and apply stroke."""
        if brush:
            self.select_brush(brush)
        if radius is not None or strength is not None:
            self.set_brush(size=radius, strength=strength)
        return self.stroke(points=points, space=space, invert=invert, pressure=pressure)

    def mask(self, op: str) -> str:
        """Mask operation: 'clear', 'invert', 'blur', 'sharpen', 'grow', 'shrink'."""
        res = self.call_tool("mask", op=op)
        return res["content"][0]["text"]

    def remesh(self, resolution: Optional[int] = None) -> str:
        """Voxel remesh the active piece."""
        args = {"resolution": resolution} if resolution is not None else {}
        res = self.call_tool("remesh", **args)
        return res["content"][0]["text"]

    def voxel_remesh(self, resolution: Optional[int] = None, voxel_size: Optional[float] = None) -> str:
        """Voxel remesh the active piece."""
        res_val = resolution
        if res_val is None and voxel_size is not None and voxel_size > 0:
            res_val = max(16, min(320, int(2.0 / voxel_size)))
        return self.remesh(resolution=res_val)

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

    def export_mesh(self, path: str, format: Optional[str] = None) -> str:
        """Export visible pieces to glTF (.glb), OBJ, STL or PLY."""
        if format and not os.path.splitext(path)[1]:
            path = f"{path}.{format.lstrip('.')}"
        return self.export_file(path=path)

    def screenshot(self) -> bytes:
        """Capture the current viewport as PNG bytes."""
        import base64
        res = self.call_tool("screenshot")
        for item in res.get("content", []):
            if item.get("type") == "image":
                return base64.b64decode(item["data"])
        raise OxyBrushError("No image returned from screenshot")

    # ---- structured read access (plugin API phase 1, RFC 0007) ----

    def _json_tool(self, _tool_name: str, **kwargs) -> Dict[str, Any]:
        res = self.call_tool(_tool_name, **kwargs)
        return json.loads(res["content"][0]["text"])

    def scene_state(self) -> Dict[str, Any]:
        """The document, pieces and camera as a dict.

        Each piece has ``topology_version``, ``edit_version`` and
        ``face_version``: if all three equal what you saw before, the piece
        hasn't changed. The camera has ``eye``, ``target``, ``up``,
        ``fov_y_deg``, ``orthographic`` and column-major ``view_matrix`` /
        ``projection_matrix`` (depth 0..1).
        """
        return self._json_tool("scene_state")

    def changes(self, since: Optional[int] = None, timeout_ms: int = 0) -> Dict[str, Any]:
        """Events after ``since`` (call without it for the starting cursor).

        Returns ``{"cursor": int, "events": [...], "truncated": bool}``.
        With ``timeout_ms`` (up to 10000) the call waits for the next change.
        When ``truncated`` is true, some events were dropped: re-read
        :meth:`scene_state`.
        """
        args: Dict[str, Any] = {"timeout_ms": int(timeout_ms)}
        if since is not None:
            args["since"] = int(since)
        return self._json_tool("changes", **args)

    def follow_changes(self, timeout_ms: int = 5000):
        """Yield events as they happen, forever (stop by breaking the loop).

        Yields ``{"kind": "truncated"}`` when events were dropped.
        """
        cursor = self.changes()["cursor"]
        while True:
            reply = self.changes(since=cursor, timeout_ms=timeout_ms)
            cursor = reply["cursor"]
            if reply.get("truncated"):
                yield {"kind": "truncated"}
            for event in reply["events"]:
                yield event

    def get_geometry(
        self,
        id: Optional[int] = None,
        index: Optional[int] = None,
        name: Optional[str] = None,
        fields: Optional[List[str]] = None,
    ) -> Dict[str, Any]:
        """A piece's arrays, read from the binary file OxyBrush writes.

        ``fields``: any of positions, normals, indices, colors, mask, groups,
        hidden (default: positions, normals, indices, plus colors if painted).
        Returns the reply (``path``, ``layout``, versions) with an
        ``arrays`` dict of flat :class:`array.array` values: positions,
        normals and colors are x, y, z per vertex; indices three per
        triangle.
        """
        import array
        import sys

        args: Dict[str, Any] = {}
        for key, value in (("id", id), ("index", index), ("name", name), ("fields", fields)):
            if value is not None:
                args[key] = value
        reply = self._json_tool("get_geometry", **args)
        with open(reply["path"], "rb") as f:
            data = f.read()
        if data[:4] != b"OXYG":
            raise OxyBrushError("not an OXYG geometry file")
        codes = {"f32": "f", "u32": "I", "u8": "B"}
        arrays = {}
        for field, sec in reply["layout"]["sections"].items():
            a = array.array(codes[sec["type"]])
            count = (reply["layout"]["vertex_count"] if sec["per"] == "vertex" else reply["layout"]["triangle_count"])
            n = count * sec["components"]
            start = sec["offset"]
            a.frombytes(data[start:start + n * a.itemsize])
            if sys.byteorder != "little" and a.itemsize > 1:
                a.byteswap()
            arrays[field] = a
        reply["arrays"] = arrays
        return reply


def connect(endpoint: Optional[str] = None, token: Optional[str] = None) -> Session:
    """Connect to a running OxyBrush instance."""
    return Session(endpoint=endpoint, token=token)


def attach() -> Session:
    """Attach to the parent OxyBrush session running this plugin script."""
    return connect()
