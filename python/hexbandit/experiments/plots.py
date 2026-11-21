"""Graphiques G1 à G5, régénérables depuis les résultats bruts.

Conventions visuelles (palettes validées pour la vision des couleurs) :

* N, grandeur ordonnée, est codé par une rampe ordinale d'un seul ton ;
* l'algorithme (UCT, MC-RAVE) par deux teintes catégorielles fixes ;
* P_premier autour de 0,5 par une échelle divergente (gris au centre) ;
* traits fins, grilles discrètes, légende dès deux séries, valeurs dans les cartes.
"""

from __future__ import annotations

import math
from collections.abc import Sequence
from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
from matplotlib.axes import Axes
from matplotlib.colors import LinearSegmentedColormap
from matplotlib.figure import Figure
from matplotlib.lines import Line2D

from hexbandit.experiments.analysis import Analysis

SURFACE = "#fcfcfb"
INK = "#0b0b0b"
INK_2 = "#52514e"
MUTED = "#898781"
GRID = "#e1e0d9"
BASELINE = "#c3c2b7"
ALGORITHM_COLORS = {"uct": "#2a78d6", "rave": "#eb6834"}
ALGORITHM_NAMES = {"uct": "UCT", "rave": "MC-RAVE"}
SIZE_RAMP = ["#86b6ef", "#5598e7", "#2a78d6", "#1c5cab", "#104281", "#0d366b"]
DIVERGING = LinearSegmentedColormap.from_list(
    "second_first", ["#1c5cab", "#6da7ec", "#f0efec", "#f2a07c", "#c44f1d"]
)
BUDGET_MARKERS = ["o", "s", "^", "D"]


def _style() -> None:
    plt.rcParams.update(
        {
            "figure.facecolor": SURFACE,
            "axes.facecolor": SURFACE,
            "savefig.facecolor": SURFACE,
            "axes.edgecolor": BASELINE,
            "axes.labelcolor": INK,
            "axes.titlecolor": INK,
            "axes.titlesize": 11,
            "axes.labelsize": 10,
            "xtick.color": INK_2,
            "ytick.color": INK_2,
            "xtick.labelsize": 9,
            "ytick.labelsize": 9,
            "text.color": INK,
            "legend.frameon": False,
            "legend.fontsize": 9,
            "font.size": 10,
            "axes.spines.top": False,
            "axes.spines.right": False,
            "axes.grid": True,
            "grid.color": GRID,
            "grid.linewidth": 0.8,
            "grid.linestyle": "-",
            "lines.linewidth": 1.6,
            "lines.solid_capstyle": "round",
            "lines.solid_joinstyle": "round",
        }
    )


def _size_colors(sizes: Sequence[int]) -> dict[int, str]:
    sizes = sorted(sizes)
    if len(sizes) == 1:
        return {sizes[0]: SIZE_RAMP[2]}
    idx = np.linspace(0, min(len(SIZE_RAMP), max(len(sizes), 2)) - 1, len(sizes))
    return {s: SIZE_RAMP[round(float(i))] for s, i in zip(sizes, idx, strict=True)}


def _ratio_label(k: float) -> str:
    k = int(k)
    return str(2**k) if k >= 0 else f"1/{2 ** (-k)}"


def _ratio_axis(
    ax: Axes, exponents: Sequence[int], label: str = "rapport de budget r = B_premier / B_second"
) -> None:
    ax.set_xticks(list(exponents))
    ax.set_xticklabels([_ratio_label(k) for k in exponents])
    ax.set_xlabel(label)


def _reference(ax: Axes, y: float = 0.5, horizontal: bool = True) -> None:
    if horizontal:
        ax.axhline(y, color=BASELINE, linewidth=1.0, zorder=1)
    else:
        ax.axvline(y, color=BASELINE, linewidth=1.0, zorder=1)


def _facets(analysis: Analysis) -> list[tuple[str, int]]:
    return [(a, b) for a in analysis.levels.algorithms for b in analysis.levels.budgets]


def _save(fig: Figure, path: Path) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(path, dpi=150, bbox_inches="tight")
    plt.close(fig)
    return path


def plot_g1(analysis: Analysis, path: Path) -> Path:
    """G1 : P_premier en fonction de log2(r), une courbe par N, facettes algorithme × B0."""
    _style()
    cells = analysis.cells
    algorithms, budgets = analysis.levels.algorithms, analysis.levels.budgets
    fig, axes = plt.subplots(
        len(algorithms),
        len(budgets),
        figsize=(5.2 * len(budgets), 3.8 * len(algorithms)),
        squeeze=False,
        sharex=True,
        sharey=True,
    )
    colors = _size_colors(analysis.levels.sizes)
    estimates = {(e.algorithm, e.base_budget, e.size): e for e in analysis.estimates}
    exponents = sorted(cells["ratio_exponent"].unique())
    grid = np.linspace(min(exponents), max(exponents), 200)
    for i, algorithm in enumerate(algorithms):
        for j, budget in enumerate(budgets):
            ax = axes[i][j]
            _reference(ax)
            _reference(ax, 0.0, horizontal=False)
            sub = cells[(cells["algorithm"] == algorithm) & (cells["base_budget"] == budget)]
            for size in analysis.levels.sizes:
                part = sub[sub["size"] == size].sort_values("ratio_exponent")
                if part.empty:
                    continue
                color = colors[size]
                est = estimates.get((algorithm, budget, size))
                if est is not None:
                    a, b_minus, b_plus = est.two_slope_params
                    eta = a + b_minus * np.minimum(grid, 0.0) + b_plus * np.maximum(grid, 0.0)
                    ax.plot(grid, 1.0 / (1.0 + np.exp(-eta)), color=color, linewidth=1.6, zorder=2)
                yerr = np.clip(
                    np.vstack(
                        [part["p_first"] - part["ci_low"], part["ci_high"] - part["p_first"]]
                    ),
                    0.0,
                    None,
                )
                ax.errorbar(
                    part["ratio_exponent"],
                    part["p_first"],
                    yerr=yerr,
                    fmt="o",
                    color=color,
                    markersize=5.5,
                    markeredgecolor=SURFACE,
                    markeredgewidth=1.2,
                    elinewidth=1.0,
                    capsize=0,
                    zorder=3,
                    label=f"N = {size}",
                )
            ax.set_ylim(-0.02, 1.02)
            ax.set_title(f"{ALGORITHM_NAMES.get(algorithm, algorithm)}, B0 = {budget} simulations")
            _ratio_axis(ax, exponents)
            if j == 0:
                ax.set_ylabel("P(le premier joueur gagne)")
    handles = [
        Line2D(
            [0],
            [0],
            color=colors[s],
            marker="o",
            markersize=6,
            markeredgecolor=SURFACE,
            linewidth=1.6,
            label=f"N = {s}",
        )
        for s in analysis.levels.sizes
    ]
    fig.suptitle(
        "G1 · Probabilité de victoire du premier joueur selon le rapport de budget\n"
        "points : fréquences observées (IC de Wilson à 95 %) ; courbes : modèle logistique à deux pentes par N",
        fontsize=11,
        x=0.01,
        ha="left",
    )
    fig.tight_layout(rect=(0.0, 0.05, 1.0, 1.0))
    fig.legend(
        handles=handles,
        loc="lower center",
        ncol=len(handles),
        title="taille du plateau",
        title_fontsize=9,
        bbox_to_anchor=(0.5, 0.0),
    )
    return _save(fig, path)


def plot_g2(analysis: Analysis, path: Path) -> Path:
    """G2 : carte de chaleur N × r de P_premier."""
    _style()
    cells = analysis.cells
    facets = _facets(analysis)
    cols = len(analysis.levels.budgets)
    rows = len(analysis.levels.algorithms)
    fig, axes = plt.subplots(
        rows, cols, figsize=(5.4 * cols, 3.6 * rows), squeeze=False, layout="constrained"
    )
    image = None
    for index, (algorithm, budget) in enumerate(facets):
        ax = axes[index // cols][index % cols]
        sub = cells[(cells["algorithm"] == algorithm) & (cells["base_budget"] == budget)]
        table = sub.pivot(index="size", columns="ratio_exponent", values="p_first").sort_index()
        image = ax.imshow(table.to_numpy(), cmap=DIVERGING, vmin=0.0, vmax=1.0, aspect="auto")
        ax.grid(False)
        ax.set_xticks(range(len(table.columns)))
        ax.set_xticklabels([_ratio_label(k) for k in table.columns])
        ax.set_yticks(range(len(table.index)))
        ax.set_yticklabels([f"N = {s}" for s in table.index])
        ax.set_xlabel("rapport de budget r")
        for (r, c), value in np.ndenumerate(table.to_numpy()):
            if np.isfinite(value):
                ink = "#ffffff" if abs(value - 0.5) > 0.32 else INK
                ax.text(c, r, f"{value:.2f}", ha="center", va="center", fontsize=8, color=ink)
        ax.set_title(f"{ALGORITHM_NAMES.get(algorithm, algorithm)}, B0 = {budget}")
        for spine in ax.spines.values():
            spine.set_visible(False)
    if image is not None:
        bar = fig.colorbar(image, ax=axes.ravel().tolist(), shrink=0.85, pad=0.01)
        bar.set_label("P(le premier joueur gagne)")
        bar.outline.set_visible(False)
        bar.ax.axhline(0.5, color=INK_2, linewidth=0.8)
    fig.suptitle(
        "G2 · P(premier joueur gagne) par taille et rapport de budget "
        "(orange : premier favorisé, bleu : second favorisé)",
        fontsize=11,
        x=0.01,
        ha="left",
    )
    return _save(fig, path)


def _series_by_algorithm(
    ax: Axes,
    frame: pd.DataFrame,
    y: str,
    low: str,
    high: str,
    budgets: Sequence[int],
    hollow: str | None = None,
) -> None:
    offsets = np.linspace(-0.18, 0.18, max(1, frame["algorithm"].nunique()))
    for o, (algorithm, part) in zip(offsets, frame.groupby("algorithm", sort=False), strict=False):
        color = ALGORITHM_COLORS.get(str(algorithm), MUTED)
        for b_index, budget in enumerate(budgets):
            sub = part[part["base_budget"] == budget].sort_values("size")
            if sub.empty:
                continue
            x = (
                sub["size"].to_numpy(dtype=float)
                + o
                + (0.06 * b_index if len(budgets) > 1 else 0.0)
            )
            yv = sub[y].to_numpy(dtype=float)
            yerr = np.vstack(
                [yv - sub[low].to_numpy(dtype=float), sub[high].to_numpy(dtype=float) - yv]
            )
            yerr = np.where(np.isfinite(yerr), np.clip(yerr, 0, None), 0.0)
            style = "-" if b_index == 0 else (0, (4, 2))
            name = ALGORITHM_NAMES.get(str(algorithm), str(algorithm))
            label = name if len(budgets) == 1 else f"{name}, B0 = {budget}"
            ax.plot(x, yv, color=color, linestyle=style, linewidth=1.6, zorder=2)
            marker = BUDGET_MARKERS[b_index % len(BUDGET_MARKERS)]
            ax.errorbar(
                x,
                yv,
                yerr=yerr,
                fmt=marker,
                color=color,
                markersize=6,
                markeredgecolor=SURFACE,
                markeredgewidth=1.2,
                elinewidth=1.0,
                capsize=0,
                zorder=3,
                label=label,
            )
            if hollow is not None:
                mask = ~sub[hollow].to_numpy(dtype=bool)
                if mask.any():
                    ax.plot(
                        x[mask],
                        yv[mask],
                        marker,
                        markersize=6,
                        markerfacecolor=SURFACE,
                        markeredgecolor=color,
                        markeredgewidth=1.4,
                        linestyle="none",
                        zorder=4,
                    )


def plot_g3(analysis: Analysis, path: Path) -> Path:
    """G3 : r* en fonction de N (modèles par groupe, IC bootstrap à 95 %)."""
    _style()
    table = analysis.estimates_table
    fig, ax = plt.subplots(figsize=(7.2, 4.4))
    _reference(ax, 0.0)
    _series_by_algorithm(
        ax,
        table,
        "log2_rstar",
        "log2_rstar_low",
        "log2_rstar_high",
        analysis.levels.budgets,
        hollow="rstar_in_range",
    )
    ticks = np.arange(-6, 7)
    ax.set_yticks(ticks)
    ax.set_yticklabels([_ratio_label(int(t)) for t in ticks])
    lo = np.nanmin(table["log2_rstar_low"].to_numpy(dtype=float)) if len(table) else -4
    hi = np.nanmax(table["log2_rstar_high"].to_numpy(dtype=float)) if len(table) else 4
    ax.set_ylim(max(-6.5, min(-1.0, lo - 0.5)), min(6.5, max(1.0, hi + 0.5)))
    ax.set_xticks(analysis.levels.sizes)
    ax.set_xlabel("taille du plateau N")
    ax.set_ylabel("ratio d'équilibre r* (échelle log2)")
    ax.legend(loc="best")
    ax.set_title(
        "G3 · Ratio d'équilibre r* (P_premier = 0,5) selon la taille\n"
        "r* < 1 : le premier joueur peut céder du budget et rester à égalité ; "
        "marqueur creux : r* extrapolé hors de la plage testée",
        fontsize=10,
        loc="left",
    )
    fig.tight_layout()
    return _save(fig, path)


def plot_g4(analysis: Analysis, path: Path) -> Path:
    """G4 : avantage du premier joueur à r = 1 en fonction de N."""
    _style()
    table = analysis.estimates_table
    fig, ax = plt.subplots(figsize=(7.2, 4.2))
    _reference(ax)
    _series_by_algorithm(
        ax, table, "p_first_r1", "p_first_r1_low", "p_first_r1_high", analysis.levels.budgets
    )
    ax.set_ylim(0.3, 1.02)
    ax.set_xticks(analysis.levels.sizes)
    ax.set_xlabel("taille du plateau N")
    ax.set_ylabel("P(premier joueur gagne) à budgets égaux")
    ax.legend(loc="best")
    ax.set_title(
        "G4 · Avantage du premier joueur à budgets égaux (r = 1), IC de Wilson à 95 %",
        fontsize=10,
        loc="left",
    )
    fig.tight_layout()
    return _save(fig, path)


def plot_g5(analysis: Analysis, path: Path) -> Path:
    """G5 : taux de victoire de Bleu à r = 1 (contrôle du biais de couleur)."""
    _style()
    cells = analysis.cells
    r1 = cells[cells["ratio_exponent"] == 0].copy()
    fig, ax = plt.subplots(figsize=(7.2, 4.0))
    _reference(ax)
    if not r1.empty:
        r1 = r1.rename(columns={"blue_rate": "y", "blue_ci_low": "lo", "blue_ci_high": "hi"})
        _series_by_algorithm(ax, r1, "y", "lo", "hi", analysis.levels.budgets)
    ax.set_ylim(0.2, 0.8)
    ax.set_xticks(analysis.levels.sizes)
    ax.set_xlabel("taille du plateau N")
    ax.set_ylabel("part des victoires de Bleu")
    ax.legend(loc="best")
    ax.set_title(
        "G5 · Victoires de Bleu à budgets égaux (r = 1) : contrôle du biais de couleur\n"
        "chaque couleur commence une partie sur deux ; attendu : 0,5",
        fontsize=10,
        loc="left",
    )
    fig.tight_layout()
    return _save(fig, path)


def plot_g6(ladders: pd.DataFrame, path: Path) -> Path:
    """G6 : gain Elo d'un doublement de budget selon le budget, une
    courbe par N, une facette par algorithme (rendements décroissants)."""
    _style()
    algorithms = [a for a in ("uct", "rave") if a in set(ladders.get("algorithm", []))]
    if not algorithms:
        fig, ax = plt.subplots(figsize=(6.0, 2.0))
        ax.axis("off")
        ax.text(0.5, 0.5, "G6 · aucun doublement de budget dans le plan", ha="center", va="center")
        return _save(fig, path)
    fig, axes = plt.subplots(
        1,
        max(1, len(algorithms)),
        figsize=(5.4 * max(1, len(algorithms)), 3.9),
        squeeze=False,
        sharey=True,
    )
    colors = _size_colors(sorted(set(ladders["size"])))
    for ax, algorithm in zip(axes[0], algorithms, strict=False):
        _reference(ax, 0.0)
        sub = ladders[ladders["algorithm"] == algorithm]
        for size, part in sub.groupby("size"):
            part = part.sort_values("budget")
            x = np.log2(part["budget"].to_numpy(dtype=float)) + 0.5
            y = part["elo_gain"].to_numpy(dtype=float)
            lo = part["elo_gain_low"].to_numpy(dtype=float)
            hi = part["elo_gain_high"].to_numpy(dtype=float)
            yerr = np.vstack([y - lo, hi - y])
            yerr = np.where(np.isfinite(yerr), np.clip(yerr, 0, None), 0.0)
            color = colors[int(size)]
            ax.plot(x, y, color=color, linewidth=1.6, zorder=2)
            ax.errorbar(
                x,
                y,
                yerr=yerr,
                fmt="o",
                color=color,
                markersize=5,
                markeredgecolor=SURFACE,
                markeredgewidth=1.0,
                elinewidth=0.9,
                capsize=0,
                zorder=3,
                label=f"N = {size}",
            )
        ticks = sorted(set(np.log2(sub["budget"].to_numpy(dtype=float)).astype(int)))
        ax.set_xticks([t + 0.5 for t in ticks[::2]])
        ax.set_xticklabels([f"{2**t}→{2 ** (t + 1)}" for t in ticks[::2]], rotation=30)
        ax.set_xlabel("doublement du budget (simulations par coup)")
        ax.set_title(ALGORITHM_NAMES.get(algorithm, algorithm))
        ax.set_ylim(-250, 750)
    axes[0][0].set_ylabel("gain Elo du doublement")
    axes[0][-1].legend(loc="upper right", ncol=2)
    fig.suptitle(
        "G6 · Valeur d'un doublement de budget (Bradley-Terry avec avantage du trait, IC bootstrap à 95 %)",
        fontsize=11,
        x=0.01,
        ha="left",
    )
    fig.tight_layout()
    return _save(fig, path)


def plot_g7(comparison: pd.DataFrame, path: Path) -> Path:
    """G7 : r* selon le modèle (deux pentes, modèle mixte, splines
    monotones, Bradley-Terry), par groupe."""
    _style()
    models = [
        ("log2_rstar", "log2_rstar_low", "log2_rstar_high", "deux pentes (V1)", "#52514e"),
        (
            "log2_rstar_mixed",
            "log2_rstar_mixed_low",
            "log2_rstar_mixed_high",
            "modèle mixte",
            "#2a78d6",
        ),
        (
            "log2_rstar_spline",
            "log2_rstar_spline_low",
            "log2_rstar_spline_high",
            "splines monotones",
            "#eb6834",
        ),
        ("log2_rstar_bt", None, None, "Bradley-Terry", "#1baf7a"),
    ]
    rows = comparison.sort_values(["algorithm", "base_budget", "size"]).reset_index(drop=True)
    fig, ax = plt.subplots(figsize=(9.5, 0.36 * len(rows) + 1.6))
    _reference(ax, 0.0, horizontal=False)
    offsets = np.linspace(-0.27, 0.27, len(models))
    for (col, lo, hi, label, color), off in zip(models, offsets, strict=True):
        if col not in rows:
            continue
        y = np.arange(len(rows)) + off
        v = rows[col].to_numpy(dtype=float)
        if lo is not None and lo in rows:
            err = np.vstack(
                [v - rows[lo].to_numpy(dtype=float), rows[hi].to_numpy(dtype=float) - v]
            )
            err = np.where(np.isfinite(err), np.clip(err, 0, None), 0.0)
            ax.errorbar(
                v,
                y,
                xerr=err,
                fmt="o",
                color=color,
                markersize=4.5,
                elinewidth=1.0,
                capsize=0,
                label=label,
            )
        else:
            ax.plot(v, y, "D", color=color, markersize=4.5, label=label, linestyle="none")
    ax.set_yticks(np.arange(len(rows)))
    ax.set_yticklabels(
        [
            f"{ALGORITHM_NAMES.get(a, a)}, B0 = {b}, N = {n}"
            for a, b, n in zip(rows["algorithm"], rows["base_budget"], rows["size"], strict=True)
        ],
        fontsize=8,
    )
    ax.invert_yaxis()
    ticks = np.arange(-3, 2)
    ax.set_xticks(ticks)
    ax.set_xticklabels([_ratio_label(int(t)) for t in ticks])
    ax.set_xlim(-3.2, 1.2)
    ax.set_xlabel("ratio d'équilibre r* (échelle log2)")
    ax.legend(loc="lower left", fontsize=8)
    ax.set_title(
        "G7 · r* selon le modèle (IC à 95 %) : les conclusions ne dépendent pas de la forme imposée",
        fontsize=10,
        loc="left",
    )
    fig.tight_layout()
    return _save(fig, path)


def plot_adaptive(summary: pd.DataFrame, steps: pd.DataFrame, path: Path) -> Path:
    """Plan adaptatif : rapports joués lot après lot et intervalle de
    crédibilité de log2 r*, un panneau par groupe."""
    _style()
    groups = list(summary.itertuples(index=False))
    cols = min(4, max(1, len(groups)))
    rows_n = math.ceil(len(groups) / cols)
    fig, axes = plt.subplots(
        rows_n, cols, figsize=(3.6 * cols, 2.8 * rows_n), squeeze=False, sharey=True
    )
    for ax, g in zip(axes.ravel(), groups, strict=False):
        part = steps[steps["group"] == f"{g.algorithm}_B{g.base_budget}_N{g.size:02d}"]
        color = ALGORITHM_COLORS.get(g.algorithm, MUTED)
        games = part["games"].cumsum().to_numpy(dtype=float)
        ax.fill_between(games, part["low"], part["high"], color=color, alpha=0.18, linewidth=0)
        ax.plot(games, part["median"], color=color, linewidth=1.4)
        ax.plot(games, part["x"], ".", color=INK_2, markersize=3.5)
        _reference(ax, 0.0)
        ax.set_title(
            f"{ALGORITHM_NAMES.get(g.algorithm, g.algorithm)}, B0 = {g.base_budget}, N = {g.size}",
            fontsize=9,
        )
        ax.set_xlabel("parties jouées", fontsize=8)
    for ax in axes.ravel()[len(groups) :]:
        ax.axis("off")
    axes[0][0].set_ylabel("log2 r")
    fig.suptitle(
        "Plan adaptatif : rapport joué à chaque lot (points) et log2 r* a posteriori (médiane, IC à 95 %)",
        fontsize=10,
        x=0.01,
        ha="left",
    )
    fig.tight_layout()
    return _save(fig, path)


def plot_all(analysis: Analysis, directory: Path) -> dict[str, Path]:
    """Produit G1 à G5 dans ``directory``."""
    directory.mkdir(parents=True, exist_ok=True)
    return {
        "G1": plot_g1(analysis, directory / "g1_p_premier_vs_ratio.png"),
        "G2": plot_g2(analysis, directory / "g2_carte_chaleur.png"),
        "G3": plot_g3(analysis, directory / "g3_ratio_equilibre.png"),
        "G4": plot_g4(analysis, directory / "g4_avantage_premier.png"),
        "G5": plot_g5(analysis, directory / "g5_biais_couleur.png"),
    }
