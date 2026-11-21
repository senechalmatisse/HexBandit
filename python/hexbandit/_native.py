"""Module natif ``hexbandit._native`` : accès bas niveau à la façade Rust.

La bibliothèque partagée (crate ``bindings``) expose une ABI C ; elle est
chargée par :mod:`ctypes`, qui libère le GIL pendant les appels.
Chaque appel est exécuté dans un thread auxiliaire : le thread principal
reste interruptible et transforme Ctrl-C en annulation propre du calcul.

Seul l'adaptateur :mod:`hexbandit.engine.native` utilise ce module ; le reste du
paquet dépend du port :class:`hexbandit.domain.ports.EnginePort`.
"""

from __future__ import annotations

import ctypes
import json
import os
import sys
import threading
from collections.abc import Callable
from pathlib import Path
from typing import Any

from hexbandit.domain.errors import Cancelled, NativeLibraryNotFound, error_from_kind

_LIB_NAMES = {
    "linux": "libhexbandit_native.so",
    "darwin": "libhexbandit_native.dylib",
    "win32": "hexbandit_native.dll",
}

_lock = threading.Lock()
_library: ctypes.CDLL | None = None


def library_path() -> Path:
    """Chemin de la bibliothèque : ``HEXBANDIT_NATIVE_LIB`` ou ``hexbandit/_lib``."""
    override = os.environ.get("HEXBANDIT_NATIVE_LIB")
    if override:
        return Path(override)
    name = _LIB_NAMES.get(sys.platform, _LIB_NAMES["linux"])
    return Path(__file__).resolve().parent / "_lib" / name


def _load() -> ctypes.CDLL:
    global _library
    with _lock:
        if _library is None:
            path = library_path()
            if not path.exists():
                raise NativeLibraryNotFound(
                    f"bibliothèque native absente ({path}) : lancer scripts/hexlXtc01.sh"
                )
            lib = ctypes.CDLL(str(path))
            for name in ("hexbandit_play_game", "hexbandit_search"):
                fn = getattr(lib, name)
                fn.argtypes = [ctypes.c_char_p]
                fn.restype = ctypes.c_void_p
            lib.hexbandit_play_batch.argtypes = [ctypes.c_char_p, ctypes.c_uint32]
            lib.hexbandit_play_batch.restype = ctypes.c_void_p
            lib.hexbandit_version.argtypes = []
            lib.hexbandit_version.restype = ctypes.c_void_p
            lib.hexbandit_cancel.argtypes = []
            lib.hexbandit_cancel.restype = None
            lib.hexbandit_free_string.argtypes = [ctypes.c_void_p]
            lib.hexbandit_free_string.restype = None
            _library = lib
        return _library


def is_available() -> bool:
    """Vrai si la bibliothèque native peut être chargée."""
    try:
        _load()
    except (NativeLibraryNotFound, OSError):
        return False
    return True


def _decode(lib: ctypes.CDLL, pointer: int | None) -> Any:
    if not pointer:
        raise error_from_kind("internal", "réponse nulle de la bibliothèque native")
    try:
        text = ctypes.string_at(pointer).decode("utf-8")
    finally:
        lib.hexbandit_free_string(pointer)
    envelope = json.loads(text)
    if envelope.get("ok"):
        return envelope["result"]
    error = envelope.get("error", {})
    raise error_from_kind(str(error.get("kind", "internal")), str(error.get("message", "")))


def _run_interruptible(lib: ctypes.CDLL, call: Callable[[], int | None]) -> Any:
    """Exécute ``call`` dans un thread ; Ctrl-C demande l'annulation au moteur."""
    if threading.current_thread() is not threading.main_thread():
        return _decode(lib, call())
    box: dict[str, Any] = {}

    def worker() -> None:
        try:
            box["pointer"] = call()
        except BaseException as exc:  # pragma: no cover - remontée telle quelle
            box["error"] = exc

    thread = threading.Thread(target=worker, name="hexbandit-native", daemon=True)
    thread.start()
    interrupted = False
    while thread.is_alive():
        try:
            thread.join(0.1)
        except KeyboardInterrupt:
            interrupted = True
            lib.hexbandit_cancel()
    if "error" in box:
        raise box["error"]
    try:
        return _decode(lib, box.get("pointer"))
    except Cancelled:
        if interrupted:
            raise KeyboardInterrupt from None
        raise


def _encode(payload: dict[str, Any]) -> bytes:
    return json.dumps(payload, ensure_ascii=False).encode("utf-8")


def version() -> dict[str, Any]:
    """Versions du moteur et des formats."""
    lib = _load()
    result: dict[str, Any] = _decode(lib, lib.hexbandit_version())
    return result


def play_game(config: dict[str, Any]) -> dict[str, Any]:
    """Joue une partie ; renvoie l'enregistrement JSON (dict)."""
    lib = _load()
    data = _encode(config)
    result: dict[str, Any] = _run_interruptible(lib, lambda: lib.hexbandit_play_game(data))
    return result


def play_batch(request: dict[str, Any], threads: int) -> dict[str, Any]:
    """Joue un lot (``games`` ou ``match``) sur ``threads`` threads."""
    lib = _load()
    data = _encode(request)
    result: dict[str, Any] = _run_interruptible(
        lib, lambda: lib.hexbandit_play_batch(data, max(1, int(threads)))
    )
    return result


def search(request: dict[str, Any]) -> dict[str, Any]:
    """Recherche sur une position ; renvoie le rapport complet."""
    lib = _load()
    data = _encode(request)
    result: dict[str, Any] = _run_interruptible(lib, lambda: lib.hexbandit_search(data))
    return result


def cancel() -> None:
    """Demande l'interruption du calcul natif en cours."""
    _load().hexbandit_cancel()
