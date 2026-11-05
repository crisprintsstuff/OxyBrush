"""
OxyBrush Python SDK: scripting and plugin library for digital sculpting.
"""

from .client import Session, OxyBrushError, MeshStats, connect, attach

__version__ = "0.5.2a0"
__all__ = ["Session", "OxyBrushError", "MeshStats", "connect", "attach"]
