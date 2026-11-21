"""Échelle Elo des budgets : modèle de Bradley-Terry avec avantage du trait.

Chaque partie d'une campagne oppose deux joueurs de même algorithme, de
budgets B_premier = r·B0 et B_second = B0. Pour chaque (algorithme, N), le
modèle donne une force θ(B) à chaque niveau de budget et un avantage γ au
joueur qui commence :

    logit P(le premier gagne) = θ(B_premier) - θ(B_second) + γ.

Les deux budgets de base (256 et 2 048) partagent des niveaux (2 048 est
joué comme r·B0 avec B0 = 256 et comme B0) : une seule échelle relie les deux
plans. θ est fixé à 0 pour le plus petit budget ; en Elo, θ·400/ln 10.

Le modèle est plus contraint que les modèles par groupe (deux paramètres par
niveau au lieu de trois par courbe) : il explique l'asymétrie des courbes
(pente b₋ > b₊) par la concavité de θ en log₂ B, c'est-à-dire par des
rendements décroissants des simulations, et donne directement le gain d'un
doublement de budget et la valeur du trait en points Elo. Le ratio
d'équilibre s'en déduit : θ(r*·B0) = θ(B0) - γ.

Le tournoi toutes rondes (:func:`round_robin_rows`) produit des parties de
même forme entre joueurs quelconques, analysées par :func:`fit_bradley_terry`.
"""

from __future__ import annotations

import math
from collections.abc import Sequence
from dataclasses import dataclass, field

import numpy as np
import pandas as pd

from hexbandit.experiments.stats import fit_logit, percentile_interval

ELO_PER_LOGIT = 400.0 / math.log(10.0)
"""Conversion d'une différence de logits en points Elo."""


@dataclass
class BradleyTerryFit:
    """Forces de Bradley-Terry d'un ensemble de joueurs."""

    players: tuple[str, ...]
    strength: np.ndarray
    strength_se: np.ndarray
    advantage: float
    advantage_se: float
    deviance: float
    df_resid: int
    games: int
    bootstrap: np.ndarray = field(repr=False, default_factory=lambda: np.empty((0, 0)))

    @property
    def elo(self) -> np.ndarray:
        """Forces en points Elo (référence : premier joueur de la liste)."""
        return self.strength * ELO_PER_LOGIT

    @property
    def advantage_elo(self) -> float:
        """Avantage du trait en points Elo."""
        return self.advantage * ELO_PER_LOGIT

    def table(self) -> pd.DataFrame:
        """Table des forces (logit et Elo, écarts-types)."""
        return pd.DataFrame(
            {
                "player": self.players,
                "theta": self.strength,
                "theta_se": self.strength_se,
                "elo": self.elo,
                "elo_se": self.strength_se * ELO_PER_LOGIT,
            }
        )


def fit_bradley_terry(
    first: Sequence[str],
    second: Sequence[str],
    first_won: Sequence[int],
    players: Sequence[str] | None = None,
    reference: str | None = None,
    n_boot: int = 0,
    seed: int = 0,
) -> BradleyTerryFit:
    """Ajuste logit P(premier gagne) = θ(premier) - θ(second) + γ.

    Les parties sont agrégées par paire ordonnée (premier, second) ; la force
    de ``reference`` (par défaut le premier joueur de ``players``) vaut 0.
    ``n_boot`` > 0 ajoute un bootstrap paramétrique des paires (intervalles des forces et de γ).
    """
    frame = pd.DataFrame({"first": first, "second": second, "won": first_won})
    names = (
        list(players) if players is not None else sorted(set(frame["first"]) | set(frame["second"]))
    )
    ref = reference if reference is not None else names[0]
    others = [p for p in names if p != ref]
    pairs = frame.groupby(["first", "second"], sort=True)["won"].agg(["sum", "size"]).reset_index()
    design = np.zeros((len(pairs), len(others) + 1))
    design[:, 0] = 1.0  # γ, avantage du trait
    index = {p: i + 1 for i, p in enumerate(others)}
    for row, (a, b) in enumerate(zip(pairs["first"], pairs["second"], strict=True)):
        if a in index:
            design[row, index[a]] += 1.0
        if b in index:
            design[row, index[b]] -= 1.0
    wins = pairs["sum"].to_numpy(dtype=float)
    n = pairs["size"].to_numpy(dtype=float)
    fit = fit_logit(design, wins, n, ["advantage", *others], firth=True)
    strength = np.zeros(len(names))
    se = np.zeros(len(names))
    for j, p in enumerate(others, start=1):
        strength[names.index(p)] = fit.params[j]
        se[names.index(p)] = fit.bse[j]
    boot: np.ndarray = np.empty((0, len(names) + 1))
    if n_boot > 0:
        rng = np.random.default_rng(seed)
        p_hat = wins / n
        samples = []
        for _ in range(n_boot):
            resampled = rng.binomial(n.astype(int), p_hat).astype(float)
            b = fit_logit(design, resampled, n, ["advantage", *others], firth=True)
            theta = np.zeros(len(names))
            for j, p in enumerate(others, start=1):
                theta[names.index(p)] = b.params[j]
            samples.append(np.r_[b.params[0], theta])
        boot = np.vstack(samples)
    return BradleyTerryFit(
        players=tuple(names),
        strength=strength,
        strength_se=se,
        advantage=float(fit.params[0]),
        advantage_se=float(fit.bse[0]),
        deviance=fit.deviance,
        df_resid=int(len(pairs) - len(others) - 1),
        games=int(n.sum()),
        bootstrap=boot,
    )


DOUBLING_COLUMNS = [
    "algorithm",
    "size",
    "budget",
    "doubled",
    "elo_gain",
    "elo_gain_low",
    "elo_gain_high",
]


@dataclass
class BudgetLadder:
    """Échelle Elo des budgets pour un (algorithme, N)."""

    algorithm: str
    size: int
    fit: BradleyTerryFit
    budgets: tuple[int, ...]

    def per_doubling(self) -> pd.DataFrame:
        """Gain Elo de chaque doublement de budget (B → 2B), avec intervalle
        bootstrap si disponible."""
        rows = []
        for low, high in zip(self.budgets[:-1], self.budgets[1:], strict=True):
            if high != 2 * low:
                continue
            i, j = self.fit.players.index(str(low)), self.fit.players.index(str(high))
            gain = (self.fit.strength[j] - self.fit.strength[i]) * ELO_PER_LOGIT
            lo, hi = float("nan"), float("nan")
            if self.fit.bootstrap.size:
                diff = (self.fit.bootstrap[:, 1 + j] - self.fit.bootstrap[:, 1 + i]) * ELO_PER_LOGIT
                lo, hi = percentile_interval(diff)
            rows.append(
                {
                    "algorithm": self.algorithm,
                    "size": self.size,
                    "budget": low,
                    "doubled": high,
                    "elo_gain": gain,
                    "elo_gain_low": lo,
                    "elo_gain_high": hi,
                }
            )
        return pd.DataFrame(rows, columns=DOUBLING_COLUMNS)

    def advantage_interval(self) -> tuple[float, float]:
        """Intervalle bootstrap de l'avantage du trait (Elo)."""
        if not self.fit.bootstrap.size:
            se = self.fit.advantage_se * ELO_PER_LOGIT
            return self.fit.advantage_elo - 1.96 * se, self.fit.advantage_elo + 1.96 * se
        return percentile_interval(self.fit.bootstrap[:, 0] * ELO_PER_LOGIT)

    def equilibrium_log2_ratio(self, base_budget: int) -> float:
        """log₂ r* implicite : budget B tel que θ(B) = θ(B0) - γ, interpolé
        linéairement en log₂ B entre niveaux (NaN hors de l'échelle)."""
        logs = np.log2(np.asarray(self.budgets, dtype=float))
        theta = np.asarray(
            [self.fit.strength[self.fit.players.index(str(b))] for b in self.budgets]
        )
        target = theta[self.budgets.index(base_budget)] - self.fit.advantage
        if not (np.all(np.diff(theta) > 0) and theta[0] <= target <= theta[-1]):
            return float("nan")
        return float(np.interp(target, theta, logs) - math.log2(base_budget))


def budget_ladders(games: pd.DataFrame, n_boot: int = 200, seed: int = 0) -> list[BudgetLadder]:
    """Échelles Elo des budgets, une par (algorithme, N), sur toutes les parties
    de la campagne (tous budgets de base confondus)."""
    ladders = []
    for (algorithm, size), group in games.groupby(["algorithm", "size"], sort=True):
        budgets = sorted(
            set(group["budget_first"].astype(int)) | set(group["budget_second"].astype(int))
        )
        names = [str(b) for b in budgets]
        fit = fit_bradley_terry(
            group["budget_first"].astype(int).astype(str).tolist(),
            group["budget_second"].astype(int).astype(str).tolist(),
            group["first_won"].astype(int).tolist(),
            players=names,
            n_boot=n_boot,
            seed=seed + int(size),
        )
        ladders.append(BudgetLadder(str(algorithm), int(size), fit, tuple(budgets)))
    return ladders


def ladders_frame(ladders: Sequence[BudgetLadder]) -> pd.DataFrame:
    """Une ligne par (algorithme, N, budget) : force et Elo."""
    rows = []
    for ladder in ladders:
        table = ladder.fit.table()
        lo, hi = ladder.advantage_interval()
        for _, r in table.iterrows():
            rows.append(
                {
                    "algorithm": ladder.algorithm,
                    "size": ladder.size,
                    "budget": int(r["player"]),
                    "elo": float(r["elo"]),
                    "elo_se": float(r["elo_se"]),
                    "first_move_elo": ladder.fit.advantage_elo,
                    "first_move_elo_low": lo,
                    "first_move_elo_high": hi,
                    "deviance": ladder.fit.deviance,
                    "df_resid": ladder.fit.df_resid,
                }
            )
    return pd.DataFrame(rows)


def round_robin_rows(
    names: Sequence[str], records_by_pair: dict[tuple[str, str], Sequence[tuple[str, str, int]]]
) -> pd.DataFrame:
    """Parties d'un tournoi sous la forme (premier, second, premier gagne)."""
    rows = [
        {"first": a, "second": b, "first_won": won}
        for pair in records_by_pair.values()
        for a, b, won in pair
    ]
    frame = pd.DataFrame(rows, columns=["first", "second", "first_won"])
    unknown = (set(frame["first"]) | set(frame["second"])) - set(names)
    if unknown:
        raise ValueError(f"joueurs inconnus dans le tournoi : {sorted(unknown)}")
    return frame
