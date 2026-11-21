"""Échantillonnage adaptatif du ratio d'équilibre r*.

La campagne à grille fixe joue toutes les cellules r = 2^k, k = -4 … 4, alors
que r* ne dépend que de la courbe autour de P = 0,5 : la plupart des parties
sont jouées loin du point d'équilibre et renseignent peu sur lui. Le plan
adaptatif joue, pour chaque groupe (algorithme, B0, N), des lots de parties
au rapport qui apporte le plus d'information sur r*, jusqu'à la précision
voulue.

* **Modèle local** : logit P(le premier gagne) = β·(x - α), x = log₂ r ;
  α = log₂ r*, β > 0. Près de l'équilibre, la courbe est localement
  logistique (l'asymétrie des deux pentes concerne surtout les extrémités).
* **A posteriori sur grille** (α, β) : α uniforme sur l'intervalle étudié,
  log β normal centré en log 1,5 (écart-type 1).
* **Choix du rapport** (méthode Psi de Kontsevich et Tyler, 1999) : parmi les
  rapports candidats, celui qui minimise l'entropie attendue de la loi a
  posteriori de α après une partie. Le budget du premier joueur est arrondi à
  l'entier : c'est le rapport effectivement joué qui entre dans le modèle.
* **Arrêt** : intervalle de crédibilité à 95 % de α plus étroit que
  ``target_width`` (après ``min_steps`` lots), ou ``max_games`` parties.

Chaque lot est un match déterministe (graine dérivée de la graine maître, du
groupe et du numéro de lot) : la procédure entière est reproductible, et une
exécution interrompue reprend à partir des lots enregistrés, en vérifiant
qu'elle refait les mêmes choix.
"""

from __future__ import annotations

import hashlib
import json
import math
import os
import time
import tomllib
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, Any, Literal

import numpy as np
from pydantic import Field, model_validator
from scipy import special

from hexbandit.domain.errors import Cancelled, ConfigError
from hexbandit.domain.types import GameSpec, MatchSpec, Seating, _Model
from hexbandit.experiments.campaign import AlgorithmTemplate

if TYPE_CHECKING:
    from hexbandit.domain.ports import EnginePort


class AdaptiveSettings(_Model):
    """Paramètres généraux d'une étude adaptative."""

    name: str
    master_seed: int = Field(ge=0, lt=2**63)
    threads: int = Field(default=2, ge=1)
    seating: Seating = "a_first_alternate_colors"
    kind: Literal["hex", "reverse_hex", "chameleon"] = "hex"
    swap: bool = False


class AdaptivePlan(_Model):
    """Groupes étudiés et règles du plan adaptatif."""

    sizes: list[int] = Field(min_length=1)
    base_budgets: list[int] = Field(min_length=1)
    algorithms: list[str] = Field(min_length=1)
    batch: int = Field(default=40, ge=2)
    max_games: int = Field(default=800, ge=2)
    min_steps: int = Field(default=3, ge=1)
    target_width: float = Field(default=0.25, gt=0.0)
    x_min: float = -5.0
    x_max: float = 3.0
    x_step: float = Field(default=0.05, gt=0.0)

    @model_validator(mode="after")
    def _check(self) -> AdaptivePlan:
        if self.batch % 2:
            raise ValueError("batch doit être pair (alternance des couleurs)")
        if not self.x_min < 0.0 < self.x_max:
            raise ValueError("l'intervalle [x_min, x_max] doit contenir 0")
        return self


class AdaptiveConfig(_Model):
    """Fichier TOML d'une étude adaptative."""

    campaign: AdaptiveSettings
    adaptive: AdaptivePlan
    players: dict[str, AlgorithmTemplate]

    @model_validator(mode="after")
    def _check(self) -> AdaptiveConfig:
        for name in self.adaptive.algorithms:
            if name not in self.players:
                raise ValueError(f"algorithme {name!r} absent de la table [players]")
        return self

    @staticmethod
    def load(path: str | Path) -> AdaptiveConfig:
        """Lit et valide un fichier TOML."""
        try:
            with Path(path).open("rb") as handle:
                return AdaptiveConfig.model_validate(tomllib.load(handle))
        except (OSError, tomllib.TOMLDecodeError, ValueError) as exc:
            raise ConfigError(f"étude adaptative {path} invalide : {exc}") from exc

    def groups(self) -> list[tuple[str, int, int]]:
        """Groupes (algorithme, B0, N) dans l'ordre d'exécution."""
        plan = self.adaptive
        return [(a, b, n) for a in plan.algorithms for b in plan.base_budgets for n in plan.sizes]


# ---------------------------------------------------------------------------
# Loi a posteriori sur grille
# ---------------------------------------------------------------------------


@dataclass
class Posterior:
    """Loi a posteriori de (α, β) sur une grille."""

    alpha: np.ndarray
    beta: np.ndarray
    log_post: np.ndarray

    @staticmethod
    def prior(x_min: float, x_max: float, resolution: float = 0.02) -> Posterior:
        """A priori : α uniforme sur [x_min - 1, x_max + 1], log β ~ N(log 1,5, 1)."""
        alpha = np.arange(x_min - 1.0, x_max + 1.0 + 1e-9, resolution)
        beta = np.geomspace(0.05, 8.0, 48)
        log_prior_beta = -0.5 * ((np.log(beta) - math.log(1.5)) / 1.0) ** 2
        log_post = np.broadcast_to(log_prior_beta, (len(alpha), len(beta))).copy()
        return Posterior(alpha, beta, log_post - special.logsumexp(log_post))

    def _p(self, x: float) -> np.ndarray:
        return np.asarray(special.expit(self.beta[None, :] * (x - self.alpha[:, None])))

    def update(self, x: float, wins: int, games: int) -> None:
        """Mise à jour par ``wins`` victoires du premier joueur sur ``games``."""
        eta = self.beta[None, :] * (x - self.alpha[:, None])
        log_lik = wins * eta - games * np.logaddexp(0.0, eta)
        self.log_post = self.log_post + log_lik
        self.log_post -= special.logsumexp(self.log_post)

    def alpha_marginal(self) -> np.ndarray:
        """Loi marginale de α."""
        return np.asarray(np.exp(special.logsumexp(self.log_post, axis=1)))

    def alpha_summary(self) -> tuple[float, float, float, float]:
        """(moyenne, médiane, borne basse, borne haute à 95 %) de α."""
        weights = self.alpha_marginal()
        cdf = np.cumsum(weights)
        mean = float(weights @ self.alpha)
        median, lo, hi = (float(np.interp(q, cdf, self.alpha)) for q in (0.5, 0.025, 0.975))
        return mean, median, lo, hi

    def best_design(self, candidates: np.ndarray) -> float:
        """Rapport minimisant l'entropie attendue de α après une partie."""
        post = np.exp(self.log_post)
        best_x, best_h = float(candidates[0]), float("inf")
        for x in candidates:
            p = self._p(float(x))
            p_win = float((post * p).sum())
            expected = 0.0
            for weight, outcome in ((p_win, p), (1.0 - p_win, 1.0 - p)):
                if weight <= 0.0:
                    continue
                joint = post * outcome / weight
                marginal = joint.sum(axis=1)
                marginal = marginal[marginal > 0]
                expected -= weight * float(marginal @ np.log(marginal))
            if expected < best_h - 1e-12:
                best_x, best_h = float(x), expected
        return best_x


# ---------------------------------------------------------------------------
# Exécution
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Step:
    """Lot de parties joué à un rapport donné."""

    step: int
    x_planned: float
    budget_first: int
    budget_second: int
    games: int
    wins: int

    @property
    def x(self) -> float:
        """Rapport effectivement joué (budget entier)."""
        return math.log2(self.budget_first / self.budget_second)


@dataclass
class GroupResult:
    """Résultat d'un groupe."""

    algorithm: str
    base_budget: int
    size: int
    steps: list[Step] = field(default_factory=list)
    complete: bool = False

    @property
    def group_id(self) -> str:
        """Identifiant stable du groupe."""
        return f"{self.algorithm}_B{self.base_budget}_N{self.size:02d}"

    @property
    def games(self) -> int:
        """Parties jouées."""
        return sum(s.games for s in self.steps)

    def posterior(self, plan: AdaptivePlan) -> Posterior:
        """Loi a posteriori après tous les lots."""
        post = Posterior.prior(plan.x_min, plan.x_max)
        for s in self.steps:
            post.update(s.x, s.wins, s.games)
        return post

    def summary(self, plan: AdaptivePlan) -> dict[str, Any]:
        """Ligne de résultat (log₂ r*, intervalle, parties)."""
        mean, median, lo, hi = self.posterior(plan).alpha_summary()
        return {
            "algorithm": self.algorithm,
            "base_budget": self.base_budget,
            "size": self.size,
            "games": self.games,
            "steps": len(self.steps),
            "log2_rstar": median,
            "log2_rstar_mean": mean,
            "log2_rstar_low": lo,
            "log2_rstar_high": hi,
            "rstar": 2.0**median,
            "width": hi - lo,
            "complete": self.complete,
        }


def _step_seed(master: int, group_id: str, step: int) -> int:
    digest = hashlib.blake2b(f"{master}:adaptive:{group_id}:{step}".encode(), digest_size=8)
    return int.from_bytes(digest.digest(), "little") & ((1 << 63) - 1)


def _candidates(plan: AdaptivePlan, base_budget: int) -> np.ndarray:
    raw = np.arange(plan.x_min, plan.x_max + 1e-9, plan.x_step)
    budgets = np.unique(np.maximum(1, np.round(base_budget * 2.0**raw)).astype(int))
    return np.log2(budgets / base_budget)


class AdaptiveRunner:
    """Exécute (ou reprend) une étude adaptative ; un fichier JSON par groupe."""

    def __init__(
        self,
        config: AdaptiveConfig,
        engine: EnginePort,
        directory: str | Path,
        progress: Callable[[str], None] | None = None,
    ) -> None:
        self.config = config
        self.engine = engine
        self.directory = Path(directory)
        self.progress = progress or (lambda message: None)
        (self.directory / "groups").mkdir(parents=True, exist_ok=True)

    def _path(self, group_id: str) -> Path:
        return self.directory / "groups" / f"{group_id}.json"

    def load(self, algorithm: str, base_budget: int, size: int) -> GroupResult:
        """Lots déjà joués d'un groupe."""
        result = GroupResult(algorithm, base_budget, size)
        path = self._path(result.group_id)
        if path.exists():
            data = json.loads(path.read_text(encoding="utf-8"))
            result.steps = [Step(**s) for s in data["steps"]]
            result.complete = bool(data.get("complete", False))
        return result

    def _save(self, result: GroupResult) -> None:
        path = self._path(result.group_id)
        tmp = path.with_suffix(".json.tmp")
        payload = {
            "group": result.group_id,
            "complete": result.complete,
            "steps": [s.__dict__ for s in result.steps],
        }
        tmp.write_text(json.dumps(payload, indent=1), encoding="utf-8")
        os.replace(tmp, path)

    def _done(self, result: GroupResult) -> bool:
        plan = self.config.adaptive
        if result.games + plan.batch > plan.max_games:
            return True
        if len(result.steps) < plan.min_steps:
            return False
        _, _, lo, hi = result.posterior(plan).alpha_summary()
        return hi - lo < plan.target_width

    def run_group(
        self, algorithm: str, base_budget: int, size: int, deadline: float | None
    ) -> GroupResult:
        """Joue les lots manquants d'un groupe (rejoue les choix enregistrés)."""
        plan = self.config.adaptive
        settings = self.config.campaign
        stored = self.load(algorithm, base_budget, size)
        result = GroupResult(algorithm, base_budget, size)
        candidates = _candidates(plan, base_budget)
        template = self.config.players[algorithm]
        while not self._done(result):
            post = result.posterior(plan)
            x = post.best_design(candidates)
            budget = max(1, round(base_budget * 2.0**x))
            index = len(result.steps)
            if index < len(stored.steps):
                old = stored.steps[index]
                if old.budget_first != budget or old.games != plan.batch:
                    raise ConfigError(
                        f"{result.group_id} : le lot {index} enregistré ({old.budget_first}) ne "
                        f"correspond pas au choix recalculé ({budget}) - configuration modifiée ?"
                    )
                result.steps.append(old)
                continue
            if deadline is not None and time.monotonic() > deadline:
                self._save(result)
                return result
            spec = MatchSpec(
                game=GameSpec(kind=settings.kind, size=size, swap=settings.swap),
                a=template.player(budget),
                b=template.player(base_budget),
                n_games=plan.batch,
                master_seed=_step_seed(settings.master_seed, result.group_id, index),
                seating=settings.seating,
            )
            records = self.engine.play_match(spec, settings.threads)
            wins = sum(1 for r in records if r.winner == r.first_player)
            result.steps.append(Step(index, x, budget, base_budget, len(records), wins))
            self._save(result)
            _, median, lo, hi = result.posterior(plan).alpha_summary()
            self.progress(
                f"{result.group_id} lot {index} : r = {budget}/{base_budget} "
                f"(log₂ {math.log2(budget / base_budget):+.2f}), premier {wins}/{len(records)} ; "
                f"log₂ r* ≈ {median:+.2f} [{lo:+.2f}, {hi:+.2f}] après {result.games} parties"
            )
        result.complete = True
        self._save(result)
        return result

    def run(self, time_limit: float | None = None) -> list[GroupResult]:
        """Exécute tous les groupes ; s'arrête proprement après ``time_limit`` s."""
        deadline = None if time_limit is None else time.monotonic() + time_limit
        results = []
        for algorithm, budget, size in self.config.groups():
            try:
                results.append(self.run_group(algorithm, budget, size, deadline))
            except Cancelled:
                self.progress("étude interrompue ; relancer pour reprendre")
                raise
            if deadline is not None and time.monotonic() > deadline:
                break
        return results

    def results(self) -> list[GroupResult]:
        """Résultats enregistrés de tous les groupes."""
        return [self.load(a, b, n) for a, b, n in self.config.groups()]


def results_frame(config: AdaptiveConfig, results: list[GroupResult]) -> Any:
    """Table des résultats (une ligne par groupe ayant au moins un lot)."""
    import pandas as pd

    return pd.DataFrame([r.summary(config.adaptive) for r in results if r.steps])
