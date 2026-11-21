"""Adaptateurs du port moteur : natif (Rust) et factice (tests)."""

from __future__ import annotations

from hexbandit.engine.fake import FakeEngine
from hexbandit.engine.native import NativeEngine, native_available

__all__ = ["FakeEngine", "NativeEngine", "native_available"]
