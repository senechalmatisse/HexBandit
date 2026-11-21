"""Domaine : types, erreurs, plateau d'affichage et ports (Protocols)."""

from __future__ import annotations

from hexbandit.domain.errors import (
    Cancelled,
    ConfigError,
    EngineInternalError,
    GameRuleError,
    HexbanditError,
    NativeLibraryNotFound,
    RecordFormatError,
    SearchFailed,
)
from hexbandit.domain.types import (
    BetaSpec,
    Color,
    GameConfig,
    GameRecord,
    GameSpec,
    MatchSpec,
    MoveRecord,
    PlayerSpec,
    PriorSpec,
    SearchReport,
    SearchRequest,
    SearchResult,
)

__all__ = [
    "BetaSpec",
    "Cancelled",
    "Color",
    "ConfigError",
    "EngineInternalError",
    "GameConfig",
    "GameRecord",
    "GameRuleError",
    "GameSpec",
    "HexbanditError",
    "MatchSpec",
    "MoveRecord",
    "NativeLibraryNotFound",
    "PlayerSpec",
    "PriorSpec",
    "RecordFormatError",
    "SearchFailed",
    "SearchReport",
    "SearchRequest",
    "SearchResult",
]
