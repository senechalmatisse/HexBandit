"""Analyse de la question scientifique.

À partir des parties (une ligne par partie) :

* table des cellules : P_premier(N, r) avec intervalle de Wilson et test
  binomial contre 0,5 ;
* modèle global du cahier des charges :
  logit(P) ~ log2(r) × N + algorithme + B0 (N catégoriel) ;
* modèles stratifiés par (algorithme, B0, N), ajustés avec correction de
  Firth : le modèle linéaire du cahier des charges, logit(P) = a + b·x avec
  x = log2(r), et un modèle à deux pentes, logit(P) = a + b₋·min(x, 0) +
  b₊·max(x, 0), qui capte l'asymétrie observée (rendements décroissants des
  simulations) ; le ratio d'équilibre r* est le point où P = 0,5 ;
* intervalles par bootstrap (rééchantillonnage des parties dans chaque cellule) ;
* tests des hypothèses H1 à H4 et contrôles de validité (biais de couleur).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

import numpy as np
import pandas as pd

from hexbandit.experiments.stats import (
    LogitFit,
    binomial_pvalue,
    fit_logit,
    likelihood_ratio_test,
    percentile_interval,
    wilson_interval,
)

GROUP = ["algorithm", "base_budget", "size"]
CELL = [*GROUP, "ratio_exponent"]


def cell_table(games: pd.DataFrame) -> pd.DataFrame:
    """Agrège les parties par cellule (algorithme, B0, N, k)."""
    if games.empty:
        raise ValueError("aucune partie à analyser")
    data = games.assign(blue_won=(games["winner"] == "blue").astype(int))
    grouped = data.groupby(CELL, sort=True)
    table = grouped.agg(
        n=("first_won", "size"),
        wins=("first_won", "sum"),
        blue_wins=("blue_won", "sum"),
        mean_moves=("num_moves", "mean"),
        mean_duration_ms=("duration_ms", "mean"),
    ).reset_index()
    table["ratio"] = 2.0 ** table["ratio_exponent"]
    table["p_first"] = table["wins"] / table["n"]
    lo, hi = wilson_interval(table["wins"], table["n"])
    table["ci_low"], table["ci_high"] = lo, hi
    table["p_value"] = [
        binomial_pvalue(int(k), int(n)) for k, n in zip(table["wins"], table["n"], strict=True)
    ]
    table["blue_rate"] = table["blue_wins"] / table["n"]
    blo, bhi = wilson_interval(table["blue_wins"], table["n"])
    table["blue_ci_low"], table["blue_ci_high"] = blo, bhi
    return table


# ---------------------------------------------------------------------------
# Modèle global
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Levels:
    """Niveaux des facteurs (le premier de chaque liste sert de référence)."""

    sizes: tuple[int, ...]
    algorithms: tuple[str, ...]
    budgets: tuple[int, ...]

    @staticmethod
    def of(cells: pd.DataFrame) -> Levels:
        """Niveaux présents dans une table de cellules."""
        algorithms = sorted(cells["algorithm"].unique(), key=lambda a: (a != "uct", a))
        return Levels(
            sizes=tuple(int(s) for s in sorted(cells["size"].unique())),
            algorithms=tuple(str(a) for a in algorithms),
            budgets=tuple(int(b) for b in sorted(cells["base_budget"].unique())),
        )


def _slope_terms(x: np.ndarray, two_slopes: bool) -> list[tuple[str, np.ndarray]]:
    if two_slopes:
        return [("log2r-", np.minimum(x, 0.0)), ("log2r+", np.maximum(x, 0.0))]
    return [("log2r", x)]


def global_design(
    cells: pd.DataFrame,
    levels: Levels,
    with_size: bool = True,
    with_algorithm: bool = True,
    algorithm_slope: bool = False,
    two_slopes: bool = False,
) -> tuple[np.ndarray, list[str]]:
    """Matrice du modèle logit(P) ~ log2(r) × N + algorithme + B0
    (``two_slopes`` : pentes distinctes pour r < 1 et r > 1)."""
    x = cells["ratio_exponent"].to_numpy(dtype=float)
    slopes = _slope_terms(x, two_slopes)
    columns: list[np.ndarray] = [np.ones(len(cells))] + [v for _, v in slopes]
    names = ["const"] + [n for n, _ in slopes]
    if with_size:
        for s in levels.sizes[1:]:
            d = (cells["size"].to_numpy() == s).astype(float)
            columns.append(d)
            names.append(f"N={s}")
            for name, v in slopes:
                columns.append(d * v)
                names.append(f"{name}:N={s}")
    if with_algorithm:
        for a in levels.algorithms[1:]:
            d = (cells["algorithm"].to_numpy() == a).astype(float)
            columns.append(d)
            names.append(f"alg={a}")
            if algorithm_slope:
                for name, v in slopes:
                    columns.append(d * v)
                    names.append(f"{name}:alg={a}")
    for b in levels.budgets[1:]:
        columns.append((cells["base_budget"].to_numpy() == b).astype(float))
        names.append(f"B0={b}")
    return np.column_stack(columns), names


def crossing(intercept: float, slope_minus: float, slope_plus: float) -> float:
    """x* tel que a + b₋·min(x,0) + b₊·max(x,0) = 0 (NaN si la pente utile est ≤ 0)."""
    slope = slope_minus if intercept >= 0 else slope_plus
    return float(-intercept / slope) if slope > 0 else float("nan")


def global_log2_rstar(fit: LogitFit, size: int, algorithm: str, budget: int) -> float:
    """log2 r* prédit par le modèle global (linéaire ou à deux pentes)."""
    eta0 = (
        fit.coef("const")
        + fit.coef(f"N={size}")
        + fit.coef(f"alg={algorithm}")
        + fit.coef(f"B0={budget}")
    )
    if "log2r" in fit.names:
        slope = fit.coef("log2r") + fit.coef(f"log2r:N={size}") + fit.coef(f"log2r:alg={algorithm}")
        return crossing(eta0, slope, slope)
    minus = fit.coef("log2r-") + fit.coef(f"log2r-:N={size}") + fit.coef(f"log2r-:alg={algorithm}")
    plus = fit.coef("log2r+") + fit.coef(f"log2r+:N={size}") + fit.coef(f"log2r+:alg={algorithm}")
    return crossing(eta0, minus, plus)


def fit_global(cells: pd.DataFrame, levels: Levels, **options: bool) -> LogitFit:
    """Ajuste le modèle global."""
    design, names = global_design(cells, levels, **options)
    return fit_logit(design, cells["wins"], cells["n"], names)


# ---------------------------------------------------------------------------
# Modèles stratifiés et bootstrap
# ---------------------------------------------------------------------------


def _design(x: np.ndarray, two_slopes: bool) -> np.ndarray:
    if two_slopes:
        return np.column_stack([np.ones_like(x), np.minimum(x, 0.0), np.maximum(x, 0.0)])
    return np.column_stack([np.ones_like(x), x])


def _group_fits(x: np.ndarray, wins: np.ndarray, n: np.ndarray) -> tuple[LogitFit, LogitFit]:
    """Modèles linéaire et à deux pentes (correction de Firth)."""
    linear = fit_logit(_design(x, False), wins, n, ["a", "b"], firth=True)
    two = fit_logit(_design(x, True), wins, n, ["a", "b-", "b+"], firth=True)
    return linear, two


def _rstars(linear: LogitFit, two: LogitFit) -> tuple[float, float]:
    a, b = float(linear.params[0]), float(linear.params[1])
    return crossing(a, b, b), crossing(
        float(two.params[0]), float(two.params[1]), float(two.params[2])
    )


@dataclass
class GroupEstimate:
    """Estimations pour un triplet (algorithme, B0, N)."""

    algorithm: str
    base_budget: int
    size: int
    intercept: float
    slope: float
    slope_p_value: float
    slope_minus: float
    slope_plus: float
    asymmetry_p_value: float
    deviance_linear: float
    deviance_two_slopes: float
    lack_of_fit_p_linear: float
    lack_of_fit_p_two_slopes: float
    log2_rstar: float
    log2_rstar_ci: tuple[float, float]
    log2_rstar_linear: float
    log2_rstar_linear_ci: tuple[float, float]
    p_first_r1: float
    p_first_r1_ci: tuple[float, float]
    blue_rate_r1: float
    blue_p_value_r1: float
    games: int
    in_range: bool
    two_slope_params: tuple[float, float, float] = (0.0, 0.0, 0.0)
    bootstrap: np.ndarray = field(repr=False, default_factory=lambda: np.empty(0))


def stratified_estimates(
    cells: pd.DataFrame, n_boot: int = 2000, seed: int = 20261115
) -> list[GroupEstimate]:
    """Ajustements par (algorithme, B0, N) et intervalles bootstrap de r*.

    r* est estimé par le modèle à deux pentes (meilleure adéquation) ; la
    valeur du modèle linéaire du cahier des charges est conservée pour comparaison."""
    from scipy import stats as sstats

    rng = np.random.default_rng(seed)
    out = []
    for (algorithm, budget, size), group in cells.groupby(GROUP, sort=True):
        group = group.sort_values("ratio_exponent")
        x = group["ratio_exponent"].to_numpy(dtype=float)
        wins = group["wins"].to_numpy(dtype=float)
        n = group["n"].to_numpy(dtype=float)
        linear, two = _group_fits(x, wins, n)
        z = linear.params[1] / linear.bse[1]
        slope_p = float(sstats.norm.sf(z))  # unilatéral : pente > 0
        point_linear, point_two = _rstars(linear, two)
        # Asymétrie : modèle à deux pentes contre modèle linéaire (1 ddl), sur
        # les vraisemblances non pénalisées.
        lin_ml = fit_logit(_design(x, False), wins, n, ["a", "b"])
        two_ml = fit_logit(_design(x, True), wins, n, ["a", "b-", "b+"])
        asym_stat = max(0.0, 2.0 * (two_ml.loglik - lin_ml.loglik))
        asym_p = float(sstats.chi2.sf(asym_stat, 1))
        df_lin, df_two = max(1, len(x) - 2), max(1, len(x) - 3)
        p_hat = wins / n
        boot = np.empty(n_boot)
        boot_linear = np.empty(n_boot)
        for i in range(n_boot):
            resampled = rng.binomial(n.astype(int), p_hat).astype(float)
            bl, bt = _group_fits(x, resampled, n)
            boot_linear[i], boot[i] = _rstars(bl, bt)
        r1 = group[group["ratio_exponent"] == 0]
        if len(r1):
            k1, n1, blue1 = (
                int(r1["wins"].iloc[0]),
                int(r1["n"].iloc[0]),
                int(r1["blue_wins"].iloc[0]),
            )
            lo, hi = wilson_interval(k1, n1)
            p_r1, ci_r1 = k1 / n1, (float(lo), float(hi))
            blue_rate, blue_p = blue1 / n1, binomial_pvalue(blue1, n1)
        else:
            p_r1, ci_r1, blue_rate, blue_p = (
                float("nan"),
                (float("nan"), float("nan")),
                float("nan"),
                float("nan"),
            )
        out.append(
            GroupEstimate(
                algorithm=str(algorithm),
                base_budget=int(budget),
                size=int(size),
                intercept=float(linear.params[0]),
                slope=float(linear.params[1]),
                slope_p_value=slope_p,
                slope_minus=float(two.params[1]),
                slope_plus=float(two.params[2]),
                asymmetry_p_value=asym_p,
                deviance_linear=lin_ml.deviance,
                deviance_two_slopes=two_ml.deviance,
                lack_of_fit_p_linear=float(sstats.chi2.sf(lin_ml.deviance, df_lin)),
                lack_of_fit_p_two_slopes=float(sstats.chi2.sf(two_ml.deviance, df_two)),
                log2_rstar=float(point_two),
                log2_rstar_ci=percentile_interval(boot),
                log2_rstar_linear=float(point_linear),
                log2_rstar_linear_ci=percentile_interval(boot_linear),
                p_first_r1=float(p_r1),
                p_first_r1_ci=ci_r1,
                blue_rate_r1=float(blue_rate),
                blue_p_value_r1=float(blue_p),
                games=int(n.sum()),
                in_range=bool(np.isfinite(point_two) and x.min() <= point_two <= x.max()),
                two_slope_params=(float(two.params[0]), float(two.params[1]), float(two.params[2])),
                bootstrap=boot,
            )
        )
    return out


def estimates_frame(estimates: list[GroupEstimate]) -> pd.DataFrame:
    """Table des estimations stratifiées."""
    rows = []
    for e in estimates:
        rows.append(
            {
                "algorithm": e.algorithm,
                "base_budget": e.base_budget,
                "size": e.size,
                "games": e.games,
                "slope": e.slope,
                "slope_p_value": e.slope_p_value,
                "slope_minus": e.slope_minus,
                "slope_plus": e.slope_plus,
                "asymmetry_p_value": e.asymmetry_p_value,
                "deviance_linear": e.deviance_linear,
                "deviance_two_slopes": e.deviance_two_slopes,
                "lack_of_fit_p_linear": e.lack_of_fit_p_linear,
                "lack_of_fit_p_two_slopes": e.lack_of_fit_p_two_slopes,
                "log2_rstar": e.log2_rstar,
                "log2_rstar_low": e.log2_rstar_ci[0],
                "log2_rstar_high": e.log2_rstar_ci[1],
                "rstar": 2.0**e.log2_rstar if np.isfinite(e.log2_rstar) else np.nan,
                "rstar_in_range": e.in_range,
                "log2_rstar_linear": e.log2_rstar_linear,
                "log2_rstar_linear_low": e.log2_rstar_linear_ci[0],
                "log2_rstar_linear_high": e.log2_rstar_linear_ci[1],
                "p_first_r1": e.p_first_r1,
                "p_first_r1_low": e.p_first_r1_ci[0],
                "p_first_r1_high": e.p_first_r1_ci[1],
                "blue_rate_r1": e.blue_rate_r1,
                "blue_p_value_r1": e.blue_p_value_r1,
            }
        )
    return pd.DataFrame(rows)


# ---------------------------------------------------------------------------
# Hypothèses
# ---------------------------------------------------------------------------


@dataclass
class Analysis:
    """Résultat complet de l'analyse."""

    cells: pd.DataFrame
    levels: Levels
    global_fit: LogitFit
    global_fit_two_slopes: LogitFit
    global_rstar: pd.DataFrame
    estimates: list[GroupEstimate]
    tests: dict[str, Any]

    @property
    def estimates_table(self) -> pd.DataFrame:
        """Estimations stratifiées sous forme de table."""
        return estimates_frame(self.estimates)


def bootstrap_global(cells: pd.DataFrame, levels: Levels, n_boot: int, seed: int) -> pd.DataFrame:
    """r* des modèles globaux (linéaire du cahier des charges et à deux pentes),
    avec intervalles bootstrap, pour chaque (N, algorithme, B0)."""
    rng = np.random.default_rng(seed)
    n = cells["n"].to_numpy(dtype=float)
    p_hat = cells["wins"].to_numpy(dtype=float) / n
    designs = {two: global_design(cells, levels, two_slopes=two) for two in (False, True)}
    fits = {two: fit_logit(d, cells["wins"], n, names) for two, (d, names) in designs.items()}
    combos = [(s, a, b) for s in levels.sizes for a in levels.algorithms for b in levels.budgets]
    samples = {two: np.empty((n_boot, len(combos))) for two in (False, True)}
    for i in range(n_boot):
        wins = rng.binomial(n.astype(int), p_hat).astype(float)
        for two, (design, names) in designs.items():
            boot_fit = fit_logit(design, wins, n, names)
            samples[two][i] = [global_log2_rstar(boot_fit, s, a, b) for s, a, b in combos]
    rows = []
    for j, (s, a, b) in enumerate(combos):
        row: dict[str, Any] = {"size": s, "algorithm": a, "base_budget": b}
        for two, suffix in ((False, "linear"), (True, "two_slopes")):
            lo, hi = percentile_interval(samples[two][:, j])
            point = global_log2_rstar(fits[two], s, a, b)
            row[f"log2_rstar_{suffix}"] = point
            row[f"log2_rstar_{suffix}_low"] = lo
            row[f"log2_rstar_{suffix}_high"] = hi
        rows.append(row)
    return pd.DataFrame(rows)


def _deviance_test(fit: LogitFit, n_cells: int) -> dict[str, float]:
    from scipy import stats as sstats

    df = n_cells - len(fit.names)
    return {
        "deviance": fit.deviance,
        "df_resid": df,
        "lack_of_fit_p": float(sstats.chi2.sf(fit.deviance, max(df, 1))),
        "converged": float(fit.converged),
    }


def analyse(games: pd.DataFrame, n_boot: int = 1000, seed: int = 20261115) -> Analysis:
    """Analyse complète : cellules, modèles, bootstrap, hypothèses H1 à H4.

    Les tests H3 et H4 sont faits avec le modèle global à deux pentes (meilleure adéquation) ;
    ceux du modèle linéaire du cahier des charges sont rapportés aussi (clés ``*_linear``)."""
    cells = cell_table(games)
    levels = Levels.of(cells)
    fit = fit_global(cells, levels)
    fit2 = fit_global(cells, levels, two_slopes=True)
    global_rstar = bootstrap_global(cells, levels, max(200, n_boot // 2), seed)
    estimates = stratified_estimates(cells, n_boot=n_boot, seed=seed + 1)
    tests: dict[str, Any] = {}

    # H1 : P_premier croît avec r (pente > 0 dans chaque groupe).
    tests["H1"] = {
        "global_slope": fit.coef("log2r"),
        "global_slope_minus": fit2.coef("log2r-"),
        "global_slope_plus": fit2.coef("log2r+"),
        "groups_positive": sum(1 for e in estimates if e.slope > 0 and e.slope_p_value < 0.05),
        "groups_both_slopes_positive": sum(
            1 for e in estimates if e.slope_minus > 0 and e.slope_plus > 0
        ),
        "groups": len(estimates),
        "groups_asymmetric": sum(
            1 for e in estimates if e.asymmetry_p_value < 0.05 and e.slope_minus > e.slope_plus
        ),
    }
    # H2 : r* < 1 (borne haute de l'intervalle de log2 r* négative).
    tests["H2"] = {
        "groups_rstar_below_1": sum(1 for e in estimates if e.log2_rstar_ci[1] < 0),
        "groups_rstar_above_1": sum(1 for e in estimates if e.log2_rstar_ci[0] > 0),
        "groups_rstar_below_1_linear": sum(1 for e in estimates if e.log2_rstar_linear_ci[1] < 0),
        "groups_rstar_above_1_linear": sum(1 for e in estimates if e.log2_rstar_linear_ci[0] > 0),
        "groups": len(estimates),
    }
    # H3 : r* varie avec N (termes en N des modèles globaux).
    if len(levels.sizes) > 1:
        stat, df, p = likelihood_ratio_test(
            fit2, fit_global(cells, levels, with_size=False, two_slopes=True)
        )
        stat_l, df_l, p_l = likelihood_ratio_test(fit, fit_global(cells, levels, with_size=False))
        tests["H3"] = {
            "lrt_stat": stat,
            "df": df,
            "p_value": p,
            "lrt_stat_linear": stat_l,
            "df_linear": df_l,
            "p_value_linear": p_l,
        }
    # H4 : RAVE modifie r* (terme algorithme, puis interaction avec les pentes).
    if len(levels.algorithms) > 1:
        stat, df, p = likelihood_ratio_test(
            fit2, fit_global(cells, levels, with_algorithm=False, two_slopes=True)
        )
        extended = fit_global(cells, levels, algorithm_slope=True, two_slopes=True)
        stat2, df2, p2 = likelihood_ratio_test(extended, fit2)
        stat_l, df_l, p_l = likelihood_ratio_test(
            fit, fit_global(cells, levels, with_algorithm=False)
        )
        tests["H4"] = {
            "lrt_stat": stat,
            "df": df,
            "p_value": p,
            "alg_coef": {a: fit2.coef(f"alg={a}") for a in levels.algorithms[1:]},
            "slope_interaction_stat": stat2,
            "slope_interaction_df": df2,
            "slope_interaction_p_value": p2,
            "lrt_stat_linear": stat_l,
            "p_value_linear": p_l,
            "differences": _rave_minus_uct(estimates),
        }
    # Contrôle : biais de couleur à r = 1, toutes cellules confondues.
    r1 = cells[cells["ratio_exponent"] == 0]
    if len(r1):
        blue, total = int(r1["blue_wins"].sum()), int(r1["n"].sum())
        tests["color_bias"] = {
            "blue_wins": blue,
            "games": total,
            "blue_rate": blue / total,
            "p_value": binomial_pvalue(blue, total),
        }
    tests["global_fit"] = _deviance_test(fit, len(cells))
    tests["global_fit_two_slopes"] = _deviance_test(fit2, len(cells))
    return Analysis(cells, levels, fit, fit2, global_rstar, estimates, tests)


def _rave_minus_uct(estimates: list[GroupEstimate]) -> list[dict[str, Any]]:
    """Différence de log2 r* (RAVE - UCT) par (B0, N), intervalle bootstrap."""
    by_key = {(e.algorithm, e.base_budget, e.size): e for e in estimates}
    out = []
    for (algorithm, budget, size), rave in by_key.items():
        if algorithm != "rave" or ("uct", budget, size) not in by_key:
            continue
        uct = by_key[("uct", budget, size)]
        m = min(len(rave.bootstrap), len(uct.bootstrap))
        diff = rave.bootstrap[:m] - uct.bootstrap[:m]
        lo, hi = percentile_interval(diff)
        out.append(
            {
                "base_budget": budget,
                "size": size,
                "difference": rave.log2_rstar - uct.log2_rstar,
                "low": lo,
                "high": hi,
            }
        )
    return out
