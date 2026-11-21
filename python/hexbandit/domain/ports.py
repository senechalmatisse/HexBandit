"""Ports de l'architecture hexagonale.

Le domaine et les cas d'usage (campagnes, analyse, visualisation) dépendent
de ces abstractions, jamais du module natif : un moteur factice
(:class:`hexbandit.engine.fake.FakeEngine`) permet de tester les campagnes sans Rust.
"""

from __future__ import annotations

from collections.abc import Sequence
from pathlib import Path
from typing import TYPE_CHECKING, Any, Protocol

from hexbandit.domain.types import GameConfig, GameRecord, MatchSpec, SearchRequest, SearchResult

if TYPE_CHECKING:
    import pandas as pd

    from hexbandit.experiments.campaign import ChunkResult


class EnginePort(Protocol):
    """Moteur de jeu : joue des parties et analyse des positions."""

    def version(self) -> dict[str, Any]:
        """Versions du moteur et des formats."""
        ...

    def play_game(self, config: GameConfig) -> GameRecord:
        """Joue une partie."""
        ...

    def play_batch(self, configs: Sequence[GameConfig], threads: int) -> list[GameRecord]:
        """Joue des parties en parallèle ; résultats dans l'ordre des configurations."""
        ...

    def play_match(self, spec: MatchSpec, threads: int) -> list[GameRecord]:
        """Joue un match (graines dérivées, alternance) en parallèle."""
        ...

    def search(self, request: SearchRequest) -> SearchResult:
        """Recherche sur une position."""
        ...


class ResultsRepository(Protocol):
    """Stockage des résultats de campagne."""

    def save(self, chunk: ChunkResult) -> None:
        """Enregistre une tranche de parties (écriture atomique)."""
        ...

    def has_chunk(self, cell_id: str, offset: int) -> bool:
        """Vrai si la tranche est déjà enregistrée (reprise après interruption)."""
        ...

    def load_all(self) -> pd.DataFrame:
        """Toutes les parties enregistrées, une ligne par partie."""
        ...


class Renderer(Protocol):
    """Rendu d'une partie enregistrée (ne dépend que du format d'enregistrement)."""

    def render_final(self, record: GameRecord, path: Path) -> Path:
        """Image de la position finale."""
        ...

    def render_frames(self, record: GameRecord, directory: Path) -> list[Path]:
        """Une image par coup."""
        ...

    def render_animation(self, record: GameRecord, path: Path, fps: float = 4.0) -> Path:
        """Animation GIF ou MP4 selon l'extension."""
        ...
