"""Adaptateur du port :class:`EnginePort` vers la bibliothèque native Rust."""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any

from hexbandit import _native
from hexbandit.domain.types import (
    SCHEMA_VERSION,
    GameConfig,
    GameRecord,
    MatchSpec,
    SearchRequest,
    SearchResult,
)


class NativeEngine:
    """Moteur natif (Rust) ; implémente :class:`hexbandit.domain.ports.EnginePort`."""

    def version(self) -> dict[str, Any]:
        """Versions du moteur et des formats."""
        return _native.version()

    def play_game(self, config: GameConfig) -> GameRecord:
        """Joue une partie."""
        return GameRecord.from_json(_native.play_game(config.to_engine()))

    def play_batch(self, configs: Sequence[GameConfig], threads: int) -> list[GameRecord]:
        """Joue des parties en parallèle (résultats dans l'ordre)."""
        request = {
            "schema_version": SCHEMA_VERSION,
            "games": [config.to_engine() for config in configs],
        }
        result = _native.play_batch(request, threads)
        return [GameRecord.from_json(r) for r in result["records"]]

    def play_match(self, spec: MatchSpec, threads: int) -> list[GameRecord]:
        """Joue un match (graines dérivées dans le moteur)."""
        result = _native.play_batch(spec.to_engine(), threads)
        return [GameRecord.from_json(r) for r in result["records"]]

    def search(self, request: SearchRequest) -> SearchResult:
        """Recherche sur une position."""
        return SearchResult.from_json(_native.search(request.to_engine()))


def native_available() -> bool:
    """Vrai si la bibliothèque native est chargeable."""
    return _native.is_available()
