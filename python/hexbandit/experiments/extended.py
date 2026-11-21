"""Analyses complémentaires, ajoutées au rapport de campagne :
échelle Elo des budgets), modèle mixte et splines monotones,
analyse de puissance, et comparaison des r* selon le modèle."""

from __future__ import annotations

import math
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np
import pandas as pd

from hexbandit.experiments.analysis import Analysis
from hexbandit.experiments.elo import BudgetLadder, budget_ladders, ladders_frame
from hexbandit.experiments.models import (
    MixedModel,
    fit_mixed_model,
    fit_monotone_splines,
    monotone_frame,
    power_analysis,
)

KEYS = ["algorithm", "base_budget", "size"]
ALGO = {"uct": "UCT", "rave": "MC-RAVE"}


@dataclass
class ExtendedAnalysis:
    """Résultats des analyses."""

    ladders: list[BudgetLadder]
    ladder_table: pd.DataFrame
    doubling: pd.DataFrame
    mixed: MixedModel
    comparison: pd.DataFrame
    power: pd.DataFrame

    def tests(self) -> dict[str, Any]:
        """Résumé numérique (ajouté à ``tests.json``)."""
        return {
            "mixed_sigma": self.mixed.sigma,
            "mixed_sigma_ci": list(self.mixed.sigma_ci),
            "mixed_lrt_stat": self.mixed.lrt_stat,
            "mixed_lrt_p_value": self.mixed.lrt_p_value,
            "first_move_elo": {
                f"{ladder.algorithm}_N{ladder.size}": ladder.fit.advantage_elo
                for ladder in self.ladders
            },
        }

    def save(self, directory: Path) -> None:
        """Tables CSV des analyses."""
        directory.mkdir(parents=True, exist_ok=True)
        self.ladder_table.to_csv(directory / "elo_budgets.csv", index=False)
        self.doubling.to_csv(directory / "elo_doublements.csv", index=False)
        self.comparison.to_csv(directory / "rstar_modeles.csv", index=False)
        self.power.to_csv(directory / "puissance.csv", index=False)


def extended_analysis(
    games: pd.DataFrame, analysis: Analysis, n_boot: int = 200, seed: int = 20261121
) -> ExtendedAnalysis:
    """Analyses complémentaires à partir des parties et de l'ancienne analyse."""
    ladders = budget_ladders(games, n_boot=n_boot, seed=seed)
    doubling = pd.concat([ladder.per_doubling() for ladder in ladders], ignore_index=True)
    mixed = fit_mixed_model(analysis.cells, seed=seed)
    splines = monotone_frame(fit_monotone_splines(analysis.cells, n_boot=n_boot, seed=seed))
    base = analysis.estimates_table[
        [*KEYS, "games", "log2_rstar", "log2_rstar_low", "log2_rstar_high"]
    ]
    bt = pd.DataFrame(
        [
            {
                "algorithm": ladder.algorithm,
                "base_budget": b0,
                "size": ladder.size,
                "log2_rstar_bt": ladder.equilibrium_log2_ratio(b0),
            }
            for ladder in ladders
            for b0 in sorted(set(base["base_budget"]))
            if b0 in ladder.budgets
        ]
    )
    comparison = (
        base.merge(mixed.table(), on=KEYS).merge(splines, on=KEYS).merge(bt, on=KEYS, how="left")
    )
    return ExtendedAnalysis(
        ladders=ladders,
        ladder_table=ladders_frame(ladders),
        doubling=doubling,
        mixed=mixed,
        comparison=comparison,
        power=power_analysis(analysis.cells),
    )


def _ratio(log2: float) -> str:
    if not math.isfinite(log2):
        return "-"
    r = 2.0**log2
    return f"{r:.2f}"


def _ci(lo: float, hi: float) -> str:
    return f"[{_ratio(lo)}, {_ratio(hi)}]"


def _table(frame: pd.DataFrame) -> str:
    header = "| " + " | ".join(str(c) for c in frame.columns) + " |"
    sep = "|" + "|".join("---" for _ in frame.columns) + "|"
    rows = ["| " + " | ".join(str(v) for v in row) + " |" for row in frame.itertuples(index=False)]
    return "\n".join([header, sep, *rows])


def extended_markdown(extended: ExtendedAnalysis, figures: dict[str, Path], rel: Any) -> str:
    """Sections du rapport consacrées aux analyses complémentaires."""
    m = extended.mixed
    parts = [
        "## Analyses complémentaires",
        "",
        "### Échelle Elo des budgets (Bradley-Terry)",
        "",
        "Pour chaque (algorithme, N), logit P(premier gagne) = θ(B_premier) - θ(B_second) + γ, "
        "sur toutes les parties (les deux budgets de base partagent des niveaux de budget). "
        "θ(B) est la force d'un budget, γ l'avantage du trait ; en points Elo, ×400/ln 10. "
        "Intervalles par bootstrap paramétrique.",
        "",
    ]
    rows = []
    for ladder in extended.ladders:
        lo, hi = ladder.advantage_interval()
        dbl = ladder.per_doubling()
        mid = dbl[(dbl["budget"] >= 128) & (dbl["budget"] <= 2048)]["elo_gain"]
        rows.append(
            {
                "algorithme": ALGO.get(ladder.algorithm, ladder.algorithm),
                "N": ladder.size,
                "avantage du trait (Elo)": f"{ladder.fit.advantage_elo:+.0f} [{lo:+.0f}, {hi:+.0f}]",
                "gain moyen d'un doublement, 128 à 4 096 sim. (Elo)": (
                    f"{mid.mean():.0f}" if len(mid) else "-"
                ),
                "déviance / ddl": f"{ladder.fit.deviance:.1f} / {ladder.fit.df_resid}",
            }
        )
    parts += [_table(pd.DataFrame(rows)), ""]
    if "G6" in figures:
        parts += [f"![G6]({rel(figures['G6'])})", ""]
    parts += [
        "### Modèle mixte et splines monotones",
        "",
        f"Modèle à deux pentes par groupe avec un effet aléatoire par cellule, u ~ N(0, σ²), σ commun : "
        f"σ = {m.sigma:.3f} [{m.sigma_ci[0]:.3f}, {m.sigma_ci[1]:.3f}] sur l'échelle logit ; test du "
        f"rapport de vraisemblance contre σ = 0 : χ² = {m.lrt_stat:.1f}, p = {m.lrt_p_value:.2g} "
        "(loi mélange ½χ²₀ + ½χ²₁). Les splines monotones (I-splines de degré 2, nœuds aux quartiles "
        "des rapports testés) n'imposent aucune forme à la courbe, seulement sa croissance.",
        "",
    ]
    c = extended.comparison.sort_values(KEYS)
    table = pd.DataFrame(
        {
            "algorithme": c["algorithm"].map(lambda a: ALGO.get(a, a)),
            "B0": c["base_budget"],
            "N": c["size"],
            "deux pentes (V1)": [
                f"{_ratio(v)} {_ci(lo, hi)}"
                for v, lo, hi in zip(
                    c["log2_rstar"], c["log2_rstar_low"], c["log2_rstar_high"], strict=True
                )
            ],
            "modèle mixte": [
                f"{_ratio(v)} {_ci(lo, hi)}"
                for v, lo, hi in zip(
                    c["log2_rstar_mixed"],
                    c["log2_rstar_mixed_low"],
                    c["log2_rstar_mixed_high"],
                    strict=True,
                )
            ],
            "splines monotones": [
                f"{_ratio(v)} {_ci(lo, hi)}"
                for v, lo, hi in zip(
                    c["log2_rstar_spline"],
                    c["log2_rstar_spline_low"],
                    c["log2_rstar_spline_high"],
                    strict=True,
                )
            ],
            "Bradley-Terry": [_ratio(v) for v in c["log2_rstar_bt"].fillna(float("nan"))],
        }
    )
    parts += [_table(table), ""]
    if "G7" in figures:
        parts += [f"![G7]({rel(figures['G7'])})", ""]
    pw = extended.power.sort_values(KEYS)
    power = pd.DataFrame(
        {
            "algorithme": pw["algorithm"].map(lambda a: ALGO.get(a, a)),
            "B0": pw["base_budget"],
            "N": pw["size"],
            "parties par cellule": pw["games_per_cell"].map(lambda v: f"{v:.0f}"),
            "écart-type de log2 r*": pw["log2_rstar_se"].map(lambda v: f"{v:.3f}"),
            "parties par cellule pour ±0,1": pw["games_per_cell_for_rstar"].map(
                lambda v: "-" if not np.isfinite(v) else f"{v:.0f}"
            ),
            "parties pour P(r = 1) ± 0,03": pw["games_for_p_r1"].map(lambda v: f"{v:.0f}"),
        }
    )
    parts += [
        "### Analyse de puissance",
        "",
        "Parties nécessaires avec la grille fixe actuelle (9 rapports) pour un intervalle à 95 % de "
        "demi-largeur 0,1 sur log2 r* (± 7 % sur r*), d'après l'information de Fisher du modèle à deux "
        "pentes (méthode delta ; la surdispersion estimée ci-dessus les augmente encore), et pour ± 3 "
        "points sur P à budgets égaux. Le plan adaptatif (`hexbandit adaptive`) vise la même précision "
        "avec beaucoup moins de parties.",
        "",
        _table(power),
        "",
    ]
    return "\n".join(parts)
