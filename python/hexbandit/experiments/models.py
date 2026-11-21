"""Modèles statistiques.

Le modèle à deux pentes de la V1 est encore rejeté par le test d'adéquation
dans 8 groupes sur 18 : avec 1 800 à 3 600 parties par groupe, de petits
écarts à la forme logistique deviennent significatifs, et les intervalles de
r* calculés comme si chaque cellule suivait exactement le modèle sont trop
étroits. Trois compléments :

* **modèle mixte** (GLMM binomial-logit-normal) : un effet aléatoire par
  cellule, u_c ~ N(0, σ²), commun à tous les groupes, absorbe la
  surdispersion ; la vraisemblance marginale est intégrée par quadrature de
  Gauss-Hermite et maximisée avec des gradients analytiques ; σ est estimé
  par vraisemblance profilée. Les intervalles de r* tiennent compte de σ ;
* **splines monotones** : logit P = a + Σ w_j I_j(x), w_j ≥ 0, avec une base
  d'I-splines (sommes cumulées de B-splines) : une courbe croissante sans
  forme imposée, pour vérifier que r* ne dépend pas du modèle paramétrique ;
* **analyse de puissance** : nombre de parties par cellule nécessaire pour
  une précision donnée sur log₂ r* ou sur P à r = 1, d'après l'information de
  Fisher du modèle à deux pentes (méthode delta).
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field

import numpy as np
import pandas as pd
from scipy import interpolate, optimize, special, stats

from hexbandit.experiments.analysis import GROUP, crossing
from hexbandit.experiments.stats import fit_logit, percentile_interval

FloatArray = np.ndarray

# ---------------------------------------------------------------------------
# Modèle mixte
# ---------------------------------------------------------------------------

_GH_NODES, _GH_WEIGHTS = np.polynomial.hermite.hermgauss(24)


def _two_slope_design(x: np.ndarray) -> np.ndarray:
    return np.column_stack([np.ones_like(x), np.minimum(x, 0.0), np.maximum(x, 0.0)])


def _marginal(
    beta: np.ndarray, sigma: float, design: np.ndarray, wins: np.ndarray, n: np.ndarray
) -> tuple[float, np.ndarray]:
    """Log-vraisemblance marginale (constante combinatoire omise) et gradient
    en β, pour un effet aléatoire N(0, σ²) par cellule."""
    eta = design @ beta
    z = np.sqrt(2.0) * sigma * _GH_NODES
    lin = eta[:, None] + z[None, :]
    log_f = wins[:, None] * lin - n[:, None] * np.logaddexp(0.0, lin)
    log_w = np.log(_GH_WEIGHTS / np.sqrt(np.pi))
    joint = log_f + log_w[None, :]
    log_l = special.logsumexp(joint, axis=1)
    post = np.exp(joint - log_l[:, None])
    mu = special.expit(lin)
    score = (post * (wins[:, None] - n[:, None] * mu)).sum(axis=1)
    return float(log_l.sum()), design.T @ score


def _fit_group(
    design: np.ndarray, wins: np.ndarray, n: np.ndarray, sigma: float, start: np.ndarray
) -> tuple[np.ndarray, float]:
    def objective(beta: np.ndarray) -> tuple[float, np.ndarray]:
        value, grad = _marginal(beta, sigma, design, wins, n)
        # Pénalité de crête très faible : estimations finies en cas de séparation.
        penalty = 1e-4 * float(beta[1:] @ beta[1:])
        return -value + penalty, -grad + 2e-4 * np.r_[0.0, beta[1:]]

    result = optimize.minimize(objective, start, jac=True, method="BFGS")
    return result.x, -float(result.fun)


def _numerical_hessian(
    fun: Callable[[np.ndarray], np.ndarray], x: np.ndarray, step: float = 1e-4
) -> np.ndarray:
    k = len(x)
    hess = np.empty((k, k))
    for i in range(k):
        e = np.zeros(k)
        e[i] = step
        hess[:, i] = (fun(x + e) - fun(x - e)) / (2 * step)
    return 0.5 * (hess + hess.T)


@dataclass
class MixedGroup:
    """r* d'un groupe selon le modèle mixte."""

    algorithm: str
    base_budget: int
    size: int
    params: np.ndarray
    cov: np.ndarray
    log2_rstar: float
    log2_rstar_ci: tuple[float, float]


@dataclass
class MixedModel:
    """Modèle mixte ajusté sur toutes les cellules."""

    sigma: float
    sigma_ci: tuple[float, float]
    loglik: float
    loglik_binomial: float
    lrt_stat: float
    lrt_p_value: float
    groups: list[MixedGroup] = field(default_factory=list)

    def table(self) -> pd.DataFrame:
        """r* par groupe (modèle mixte)."""
        return pd.DataFrame(
            [
                {
                    "algorithm": g.algorithm,
                    "base_budget": g.base_budget,
                    "size": g.size,
                    "log2_rstar_mixed": g.log2_rstar,
                    "log2_rstar_mixed_low": g.log2_rstar_ci[0],
                    "log2_rstar_mixed_high": g.log2_rstar_ci[1],
                }
                for g in self.groups
            ]
        )


def fit_mixed_model(cells: pd.DataFrame, n_draws: int = 4000, seed: int = 0) -> MixedModel:
    """Ajuste le modèle à deux pentes par groupe avec un effet aléatoire par
    cellule (σ commun), par vraisemblance profilée en σ."""
    groups = [(key, g.sort_values("ratio_exponent")) for key, g in cells.groupby(GROUP, sort=True)]
    data = []
    for key, g in groups:
        x = g["ratio_exponent"].to_numpy(dtype=float)
        design = _two_slope_design(x)
        wins = g["wins"].to_numpy(dtype=float)
        n = g["n"].to_numpy(dtype=float)
        start = fit_logit(design, wins, n, ["a", "b-", "b+"], firth=True).params
        data.append((key, design, wins, n, start))

    def profile(log_sigma: float) -> float:
        sigma = float(np.exp(log_sigma))
        return -sum(_fit_group(d, w, n, sigma, s)[1] for _, d, w, n, s in data)

    best = optimize.minimize_scalar(profile, bounds=(np.log(1e-3), np.log(3.0)), method="bounded")
    sigma = float(np.exp(best.x))
    loglik = -float(best.fun)
    loglik0 = -profile(np.log(1e-6))
    # Intervalle de σ par la vraisemblance profilée (seuil χ²₁ / 2).
    threshold = loglik - 0.5 * float(stats.chi2.ppf(0.95, 1))

    def gap(log_sigma: float) -> float:
        return -profile(log_sigma) - threshold

    lo = hi = float("nan")
    grid = np.linspace(np.log(1e-3), np.log(3.0), 40)
    values = np.array([gap(v) for v in grid])
    inside = values >= 0
    if inside.any():
        first, last = np.argmax(inside), len(inside) - 1 - np.argmax(inside[::-1])
        lo = (
            0.0 if first == 0 else float(np.exp(optimize.brentq(gap, grid[first - 1], grid[first])))
        )
        hi = (
            float(np.exp(grid[-1]))
            if last == len(grid) - 1
            else float(np.exp(optimize.brentq(gap, grid[last], grid[last + 1])))
        )
    stat = max(0.0, 2.0 * (loglik - loglik0))
    model = MixedModel(
        sigma=sigma,
        sigma_ci=(lo, hi),
        loglik=loglik,
        loglik_binomial=loglik0,
        lrt_stat=stat,
        # Test à la frontière (σ = 0) : mélange ½ χ²₀ + ½ χ²₁.
        lrt_p_value=float(0.5 * stats.chi2.sf(stat, 1)),
    )
    rng = np.random.default_rng(seed)
    for (algorithm, budget, size), d, w, n, s in data:
        params, _ = _fit_group(d, w, n, sigma, s)

        def grad(
            beta: np.ndarray, d: np.ndarray = d, w: np.ndarray = w, n: np.ndarray = n
        ) -> np.ndarray:
            return -_marginal(beta, sigma, d, w, n)[1]

        hess = _numerical_hessian(grad, params)
        try:
            cov = np.linalg.inv(hess)
        except np.linalg.LinAlgError:
            cov = np.linalg.pinv(hess)
        cov = 0.5 * (cov + cov.T)
        draws = rng.multivariate_normal(params, cov, size=n_draws, check_valid="ignore")
        rstars = np.array([crossing(a, bm, bp) for a, bm, bp in draws])
        model.groups.append(
            MixedGroup(
                algorithm=str(algorithm),
                base_budget=int(budget),
                size=int(size),
                params=params,
                cov=cov,
                log2_rstar=crossing(float(params[0]), float(params[1]), float(params[2])),
                log2_rstar_ci=percentile_interval(rstars),
            )
        )
    return model


# ---------------------------------------------------------------------------
# Splines monotones
# ---------------------------------------------------------------------------


def ispline_basis(
    x: np.ndarray, knots: np.ndarray, lower: float, upper: float, degree: int = 2
) -> np.ndarray:
    """Base d'I-splines croissantes de 0 à 1 sur [lower, upper] : sommes
    cumulées (de droite à gauche) d'une base de B-splines de degré
    ``degree``, dont la somme vaut 1."""
    t = np.r_[[lower] * (degree + 1), np.asarray(knots, dtype=float), [upper] * (degree + 1)]
    count = len(t) - degree - 1
    xx = np.clip(np.asarray(x, dtype=float), lower, upper - 1e-12)
    b = np.empty((len(xx), count))
    for j in range(count):
        coef = np.zeros(count)
        coef[j] = 1.0
        b[:, j] = interpolate.BSpline(t, coef, degree, extrapolate=False)(xx)
    b = np.nan_to_num(b)
    # I_j = Σ_{m ≥ j} B_m pour j ≥ 1 (I_0 ≡ 1 est l'ordonnée à l'origine).
    return np.cumsum(b[:, ::-1], axis=1)[:, ::-1][:, 1:]


@dataclass
class MonotoneFit:
    """Courbe logistique monotone d'un groupe."""

    algorithm: str
    base_budget: int
    size: int
    intercept: float
    weights: np.ndarray
    knots: np.ndarray
    lower: float
    upper: float
    log2_rstar: float
    log2_rstar_ci: tuple[float, float]
    deviance: float

    def eta(self, x: np.ndarray) -> np.ndarray:
        """logit P en x = log₂ r."""
        basis = ispline_basis(x, self.knots, self.lower, self.upper)
        return np.asarray(self.intercept + basis @ self.weights)


def _fit_monotone(
    x: np.ndarray, wins: np.ndarray, n: np.ndarray, knots: np.ndarray, lower: float, upper: float
) -> tuple[float, np.ndarray]:
    basis = ispline_basis(x, knots, lower, upper)
    k = basis.shape[1]
    ridge = 1e-3

    def objective(theta: np.ndarray) -> tuple[float, np.ndarray]:
        eta = theta[0] + basis @ theta[1:]
        mu = special.expit(eta)
        value = -float(np.sum(wins * eta - n * np.logaddexp(0.0, eta)))
        resid = n * mu - wins
        grad = np.r_[resid.sum(), basis.T @ resid]
        return value + ridge * float(theta[1:] @ theta[1:]), grad + np.r_[
            0.0, 2 * ridge * theta[1:]
        ]

    start = np.r_[-1.0, np.full(k, 0.5)]
    result = optimize.minimize(
        objective,
        start,
        jac=True,
        method="L-BFGS-B",
        bounds=[(None, None)] + [(0.0, None)] * k,
    )
    return float(result.x[0]), np.asarray(result.x[1:])


def _root(eta: Callable[[np.ndarray], np.ndarray], lower: float, upper: float) -> float:
    def at(v: float) -> float:
        return float(eta(np.array([v]))[0])

    if at(lower) > 0 or at(upper) < 0:
        return float("nan")
    return float(optimize.brentq(at, lower, upper))


def fit_monotone_splines(
    cells: pd.DataFrame, n_boot: int = 200, seed: int = 0
) -> list[MonotoneFit]:
    """Courbes logistiques monotones (I-splines) par groupe, r* et intervalle
    par bootstrap paramétrique des cellules."""
    rng = np.random.default_rng(seed)
    fits = []
    for (algorithm, budget, size), g in cells.groupby(GROUP, sort=True):
        g = g.sort_values("ratio_exponent")
        x = g["ratio_exponent"].to_numpy(dtype=float)
        wins = g["wins"].to_numpy(dtype=float)
        n = g["n"].to_numpy(dtype=float)
        lower, upper = float(x.min()), float(x.max())
        knots = np.quantile(x, [0.25, 0.5, 0.75])
        a, w = _fit_monotone(x, wins, n, knots, lower, upper)
        fit = MonotoneFit(
            str(algorithm),
            int(budget),
            int(size),
            a,
            w,
            knots,
            lower,
            upper,
            float("nan"),
            (float("nan"), float("nan")),
            0.0,
        )
        fit.log2_rstar = _root(fit.eta, lower, upper)
        mu = np.clip(special.expit(fit.eta(x)), 1e-12, 1 - 1e-12)
        with np.errstate(divide="ignore", invalid="ignore"):
            sat = np.where(wins > 0, wins * np.log(wins / (n * mu)), 0.0) + np.where(
                n - wins > 0, (n - wins) * np.log((n - wins) / (n * (1 - mu))), 0.0
            )
        fit.deviance = float(2.0 * sat.sum())
        p_hat = wins / n
        boots = []
        for _ in range(n_boot):
            resampled = rng.binomial(n.astype(int), p_hat).astype(float)
            ab, wb = _fit_monotone(x, resampled, n, knots, lower, upper)
            boot_fit = MonotoneFit(
                fit.algorithm,
                fit.base_budget,
                fit.size,
                ab,
                wb,
                knots,
                lower,
                upper,
                float("nan"),
                (float("nan"), float("nan")),
                0.0,
            )
            boots.append(_root(boot_fit.eta, lower, upper))
        fit.log2_rstar_ci = percentile_interval(np.asarray(boots))
        fits.append(fit)
    return fits


def monotone_frame(fits: list[MonotoneFit]) -> pd.DataFrame:
    """r* par groupe (splines monotones)."""
    return pd.DataFrame(
        [
            {
                "algorithm": f.algorithm,
                "base_budget": f.base_budget,
                "size": f.size,
                "log2_rstar_spline": f.log2_rstar,
                "log2_rstar_spline_low": f.log2_rstar_ci[0],
                "log2_rstar_spline_high": f.log2_rstar_ci[1],
                "deviance_spline": f.deviance,
            }
            for f in fits
        ]
    )


# ---------------------------------------------------------------------------
# Analyse de puissance
# ---------------------------------------------------------------------------


def power_analysis(
    cells: pd.DataFrame, rstar_halfwidth: float = 0.1, p_halfwidth: float = 0.03
) -> pd.DataFrame:
    """Parties par cellule nécessaires (plan à grille fixe actuel) pour un
    intervalle à 95 % de demi-largeur ``rstar_halfwidth`` sur log₂ r* et
    ``p_halfwidth`` sur P à r = 1.

    La variance de log₂ r* est obtenue par la méthode delta à partir de
    l'information de Fisher du modèle à deux pentes ajusté : multiplier le
    nombre de parties de chaque cellule par s la divise par s.
    """
    z = float(stats.norm.ppf(0.975))
    rows = []
    for (algorithm, budget, size), g in cells.groupby(GROUP, sort=True):
        x = g["ratio_exponent"].to_numpy(dtype=float)
        wins = g["wins"].to_numpy(dtype=float)
        n = g["n"].to_numpy(dtype=float)
        fit = fit_logit(_two_slope_design(x), wins, n, ["a", "b-", "b+"], firth=True)
        a, bm, bp = (float(v) for v in fit.params)
        slope, col = (bm, 1) if a >= 0 else (bp, 2)
        grad = np.zeros(3)
        if slope > 0:
            grad[0] = -1.0 / slope
            grad[col] = a / slope**2
        var = float(grad @ fit.cov @ grad) if slope > 0 else float("nan")
        se = float(np.sqrt(var)) if np.isfinite(var) else float("nan")
        per_cell = float(n.mean())
        scale = (z * se / rstar_halfwidth) ** 2 if np.isfinite(se) else float("nan")
        r1 = g[g["ratio_exponent"] == 0]
        p1 = float(r1["wins"].iloc[0] / r1["n"].iloc[0]) if len(r1) else float("nan")
        rows.append(
            {
                "algorithm": algorithm,
                "base_budget": int(budget),
                "size": int(size),
                "games_per_cell": per_cell,
                "log2_rstar_se": se,
                "games_per_cell_for_rstar": float(np.ceil(scale * per_cell)),
                "games_for_rstar_total": float(np.ceil(scale * n.sum())),
                "games_for_p_r1": float(np.ceil(z**2 * p1 * (1 - p1) / p_halfwidth**2)),
            }
        )
    return pd.DataFrame(rows)
