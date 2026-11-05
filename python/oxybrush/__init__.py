"""
OxyBrush Python SDK: scripting and plugin library for digital sculpting.
"""

from .client import Session, OxyBrushError, connect, attach

__version__ = "0.4.0a0"
__all__ = ["Session", "OxyBrushError", "connect", "attach"]
