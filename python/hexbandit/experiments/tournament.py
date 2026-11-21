"""Tournoi toutes rondes.

Les joueurs sont décrits dans un fichier TOML ; chaque paire joue
``games_per_pair`` parties, chaque joueur commençant la moitié d'entre elles.
Les résultats sont analysés par le modèle de Bradley-Terry avec avantage du
trait (:mod:`hexbandit.experiments.elo`) : classement en points Elo.

Exemple ::

    [tournament]
    name = "reglages-9x9"
    size = 9
    games_per_pair = 100
    master_seed = 2026

    [players.uct]
    algorithm = "uct"
    simulations = 1000
    exploration = 0.25

    [players.rave]
    algorithm = "rave"
    simulations = 1000
"""

from __future__ import annotations

import hashlib
import itertools
import tomllib
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Literal

import pandas as pd
from pydantic import Field, model_validator

from hexbandit.domain.errors import ConfigError
from hexbandit.domain.types import GameSpec, MatchSpec, PlayerSpec, _Model
from hexbandit.experiments.elo import BradleyTerryFit, fit_bradley_terry

if TYPE_CHECKING:
    from hexbandit.domain.ports import EnginePort


class TournamentSettings(_Model):
    """Paramètres d'un tournoi."""

    name: str
    size: int = Field(default=9, ge=2, le=19)
    kind: Literal["hex", "reverse_hex", "chameleon", "quarto", "quarto_squares"] = "hex"
    swap: bool = False
    games_per_pair: int = Field(default=100, ge=2)
    master_seed: int = Field(default=0, ge=0, lt=2**63)
    threads: int = Field(default=2, ge=1)

    @model_validator(mode="after")
    def _check(self) -> TournamentSettings:
        if self.games_per_pair % 2:
            raise ValueError(
                "games_per_pair doit être pair (chaque joueur commence une partie sur deux)"
            )
        return self


class TournamentConfig(_Model):
    """Fichier TOML d'un tournoi."""

    tournament: TournamentSettings
    players: dict[str, PlayerSpec]

    @model_validator(mode="after")
    def _check(self) -> TournamentConfig:
        if len(self.players) < 2:
            raise ValueError("au moins deux joueurs")
        return self

    @staticmethod
    def load(path: str | Path) -> TournamentConfig:
        """Lit et valide un fichier TOML."""
        try:
            with Path(path).open("rb") as handle:
                return TournamentConfig.model_validate(tomllib.load(handle))
        except (OSError, tomllib.TOMLDecodeError, ValueError) as exc:
            raise ConfigError(f"tournoi {path} invalide : {exc}") from exc

    def game(self) -> GameSpec:
        """Jeu du tournoi."""
        t = self.tournament
        if t.kind.startswith("quarto"):
            return GameSpec(kind=t.kind)
        return GameSpec(kind=t.kind, size=t.size, swap=t.swap)


@dataclass
class TournamentResult:
    """Parties et classement d'un tournoi."""

    games: pd.DataFrame
    fit: BradleyTerryFit

    def standings(self) -> pd.DataFrame:
        """Classement (Elo décroissant), avec bilan de chaque joueur."""
        table = self.fit.table()
        wins = []
        played = []
        for name in table["player"]:
            mine = self.games[(self.games["first"] == name) | (self.games["second"] == name)]
            won = ((mine["first"] == name) & (mine["first_won"] == 1)) | (
                (mine["second"] == name) & (mine["first_won"] == 0)
            )
            wins.append(int(won.sum()))
            played.append(len(mine))
        table["games"] = played
        table["wins"] = wins
        table["elo"] -= table["elo"].mean()
        return table.sort_values("elo", ascending=False).reset_index(drop=True)


def _pair_seed(master: int, a: str, b: str) -> int:
    digest = hashlib.blake2b(f"{master}:tournament:{a}:{b}".encode(), digest_size=8).digest()
    return int.from_bytes(digest, "little") & ((1 << 63) - 1)


def run_tournament(
    config: TournamentConfig,
    engine: EnginePort,
    progress: Callable[[str], None] | None = None,
    n_boot: int = 200,
) -> TournamentResult:
    """Joue toutes les paires et ajuste le classement."""
    progress = progress or (lambda message: None)
    t = config.tournament
    names = list(config.players)
    rows = []
    for a, b in itertools.combinations(names, 2):
        spec = MatchSpec(
            game=config.game(),
            a=config.players[a],
            b=config.players[b],
            n_games=t.games_per_pair,
            master_seed=_pair_seed(t.master_seed, a, b),
            seating="alternate_first",
        )
        records = engine.play_match(spec, t.threads)
        wins_a = 0
        for r in records:
            a_first = r.a_color == r.first_player
            first, second = (a, b) if a_first else (b, a)
            rows.append(
                {"first": first, "second": second, "first_won": int(r.winner == r.first_player)}
            )
            wins_a += int(r.winner == r.a_color)
        progress(f"{a} contre {b} : {wins_a}/{len(records)} pour {a}")
    games = pd.DataFrame(rows)
    fit = fit_bradley_terry(
        games["first"].tolist(),
        games["second"].tolist(),
        games["first_won"].tolist(),
        players=names,
        reference=_reference(games, names),
        n_boot=n_boot,
        seed=t.master_seed,
    )
    return TournamentResult(games, fit)


def _reference(games: pd.DataFrame, names: list[str]) -> str:
    """Joueur de référence du classement (force 0) : celui dont le taux de
    victoire est le plus proche de 50 %. Les écarts-types des autres forces
    sont ceux de leur écart à la référence ; un joueur qui perd ou gagne
    presque tout serait une référence très imprécise."""

    def balance(name: str) -> float:
        as_first = games["first"] == name
        as_second = games["second"] == name
        won = int(games.loc[as_first, "first_won"].sum()) + int(
            (1 - games.loc[as_second, "first_won"]).sum()
        )
        played = int(as_first.sum() + as_second.sum())
        return abs(won / played - 0.5) if played else 1.0

    return min(names, key=lambda name: (balance(name), names.index(name)))
