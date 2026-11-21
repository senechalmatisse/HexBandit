"""Moteur factice : implémente :class:`EnginePort` sans Rust.

Le vainqueur de chaque partie est tiré selon un modèle connu,
logit P(premier gagne) = a(N) + b·log2(B_premier / B_second), ce qui permet de
tester toute la chaîne campagne → statistiques → graphiques et de vérifier
que l'analyse retrouve les paramètres du modèle.
"""

from __future__ import annotations

import hashlib
from collections.abc import Callable, Sequence
from typing import Any

import numpy as np

from hexbandit.domain.hexboard import format_cell
from hexbandit.domain.types import (
    Color,
    GameConfig,
    GameRecord,
    MatchSpec,
    MoveRecord,
    PlayerSpec,
    SearchReport,
    SearchRequest,
    SearchResult,
    other_color,
)

ProbabilityModel = Callable[[int, float, str], float]


def default_model(size: int, log2_ratio: float, algorithm: str) -> float:
    """Modèle par défaut : avantage du premier joueur croissant avec N."""
    intercept = 0.2 + 0.08 * size + (0.3 if algorithm == "rave" else 0.0)
    slope = 0.45
    return float(1.0 / (1.0 + np.exp(-(intercept + slope * log2_ratio))))


def _budget(spec: PlayerSpec) -> float:
    return float(spec.simulations or spec.time_ms or 1)


def _seed(master: int, index: int) -> int:
    digest = hashlib.blake2b(f"{master}:{index}".encode(), digest_size=8).digest()
    return int.from_bytes(digest, "little")


class FakeEngine:
    """Moteur déterministe sans simulation de jeu."""

    def __init__(self, model: ProbabilityModel = default_model) -> None:
        self.model = model
        self.calls = 0

    def version(self) -> dict[str, Any]:
        """Versions factices."""
        return {"engine_version": "fake", "record_format_version": 1}

    def _record(
        self,
        size: int,
        first: Color,
        players: dict[Color, PlayerSpec],
        seed: int,
        index: int | None = None,
        a_color: Color | None = None,
    ) -> GameRecord:
        rng = np.random.default_rng(seed)
        first_spec, second_spec = players[first], players[other_color(first)]
        log2_ratio = float(np.log2(_budget(first_spec) / _budget(second_spec)))
        p_first = self.model(size, log2_ratio, first_spec.algorithm)
        winner: Color = first if rng.random() < p_first else other_color(first)
        cells = rng.permutation(size * size)
        length = int(rng.integers(size, size * size + 1))
        # Le dernier coup est celui du vainqueur : la parité est ajustée.
        last_is_first = length % 2 == 1
        if last_is_first != (winner == first):
            length = length - 1 if length > size else length + 1
        moves = []
        color = first
        for ply in range(1, length + 1):
            cell = int(cells[ply - 1])
            moves.append(
                MoveRecord(ply=ply, player=color, move=format_cell(cell // size, cell % size))
            )
            color = other_color(color)
        return GameRecord(
            kind="hex",
            size=size,
            swap=False,
            first_player=first,
            seed=seed,
            players={c: p.to_engine() for c, p in players.items()},
            moves=tuple(moves),
            winner=winner,
            duration_ms=0.0,
            engine_version="fake",
            match_index=index,
            a_color=a_color,
        )

    def play_game(self, config: GameConfig) -> GameRecord:
        """Partie factice."""
        self.calls += 1
        players: dict[Color, PlayerSpec] = {"blue": config.blue, "red": config.red}
        return self._record(config.game.size, config.first_player, players, config.seed)

    def play_batch(self, configs: Sequence[GameConfig], threads: int) -> list[GameRecord]:
        """Parties factices."""
        return [self.play_game(c) for c in configs]

    def play_match(self, spec: MatchSpec, threads: int) -> list[GameRecord]:
        """Match factice, avec la même règle de placement que le moteur."""
        self.calls += 1
        records = []
        for i in range(spec.n_games):
            index = spec.offset + i
            a_color: Color = "blue" if index % 2 == 0 else "red"
            first: Color = a_color if spec.seating == "a_first_alternate_colors" else "blue"
            players: dict[Color, PlayerSpec] = {a_color: spec.a, other_color(a_color): spec.b}
            seed = _seed(spec.master_seed, index)
            records.append(self._record(spec.game.size, first, players, seed, index, a_color))
        return records

    def search(self, request: SearchRequest) -> SearchResult:
        """Recherche factice : premier coup légal."""
        report = SearchReport(
            simulations=0,
            nodes=1,
            max_depth=0,
            mean_depth=0.0,
            root_value=0.5,
            best_visits=0,
            best_value=0.5,
            node_limit_reached=False,
            elapsed_ms=0.0,
        )
        return SearchResult(
            position="",
            player=request.first_player,
            best_move="a1",
            report=report,
            player_config=request.player.to_engine(),
        )
