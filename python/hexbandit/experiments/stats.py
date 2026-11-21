"""Statistiques : intervalle de Wilson, test binomial, régression
logistique binomiale (IRLS, avec correction de Firth optionnelle), test du
rapport de vraisemblance et bootstrap.

La régression est implémentée avec NumPy/SciPy (statsmodels n'étant pas
disponible dans l'environnement de développement) ; elle est vérifiée contre
une optimisation directe de la vraisemblance et, si statsmodels est installé,
contre ``statsmodels.GLM`` (tests).
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass, field

import numpy as np
from numpy.typing import ArrayLike, NDArray
from scipy import special, stats

FloatArray = NDArray[np.float64]


def wilson_interval(
    successes: ArrayLike, trials: ArrayLike, confidence: float = 0.95
) -> tuple[FloatArray, FloatArray]:
    """Intervalle de score de Wilson pour une proportion binomiale."""
    k = np.asarray(successes, dtype=float)
    n = np.asarray(trials, dtype=float)
    z = float(stats.norm.ppf(0.5 + confidence / 2.0))
    with np.errstate(invalid="ignore", divide="ignore"):
        p = np.where(n > 0, k / n, np.nan)
        denom = 1.0 + z * z / n
        center = (p + z * z / (2.0 * n)) / denom
        half = z * np.sqrt(p * (1.0 - p) / n + z * z / (4.0 * n * n)) / denom
    return np.clip(center - half, 0.0, 1.0), np.clip(center + half, 0.0, 1.0)


def binomial_pvalue(
    successes: int, trials: int, p: float = 0.5, alternative: str = "two-sided"
) -> float:
    """p-valeur exacte du test binomial (``two-sided``, ``greater``, ``less``)."""
    if trials <= 0:
        return float("nan")
    return float(stats.binomtest(int(successes), int(trials), p, alternative=alternative).pvalue)


@dataclass(frozen=True)
class LogitFit:
    """Régression logistique binomiale ajustée."""

    names: tuple[str, ...]
    params: FloatArray
    cov: FloatArray
    loglik: float
    deviance: float
    n_obs: int
    converged: bool
    iterations: int
    firth: bool = False
    extra: dict[str, float] = field(default_factory=dict)

    @property
    def bse(self) -> FloatArray:
        """Écarts-types des coefficients."""
        return np.sqrt(np.clip(np.diag(self.cov), 0.0, None))

    def index(self, name: str) -> int:
        """Position d'un coefficient."""
        return self.names.index(name)

    def coef(self, name: str) -> float:
        """Valeur d'un coefficient (0 s'il n'existe pas)."""
        return float(self.params[self.index(name)]) if name in self.names else 0.0

    def wald(self, name: str) -> tuple[float, float]:
        """Statistique z et p-valeur bilatérale du test de Wald."""
        i = self.index(name)
        z = float(self.params[i] / self.bse[i])
        return z, float(2.0 * stats.norm.sf(abs(z)))

    def predict(self, design: ArrayLike) -> FloatArray:
        """Probabilités prédites pour une matrice de plan."""
        return np.asarray(special.expit(np.asarray(design, dtype=float) @ self.params), dtype=float)

    def summary_rows(self) -> list[tuple[str, float, float, float, float]]:
        """(nom, coefficient, écart-type, z, p) pour chaque coefficient."""
        rows = []
        for i, name in enumerate(self.names):
            z = float(self.params[i] / self.bse[i]) if self.bse[i] > 0 else float("nan")
            rows.append(
                (
                    name,
                    float(self.params[i]),
                    float(self.bse[i]),
                    z,
                    float(2 * stats.norm.sf(abs(z))),
                )
            )
        return rows


def _binomial_loglik(successes: FloatArray, trials: FloatArray, eta: FloatArray) -> float:
    # log L = Σ k·η - n·log(1 + e^η) (+ constante combinatoire)
    return float(np.sum(successes * eta - trials * np.logaddexp(0.0, eta)))


def _saturated_loglik(successes: FloatArray, trials: FloatArray) -> float:
    with np.errstate(divide="ignore", invalid="ignore"):
        p = np.where(trials > 0, successes / trials, 0.5)
        term = np.where(successes > 0, successes * np.log(np.where(p > 0, p, 1.0)), 0.0)
        term += np.where(
            trials - successes > 0, (trials - successes) * np.log(np.where(p < 1, 1 - p, 1.0)), 0.0
        )
    return float(np.sum(term))


def fit_logit(
    design: ArrayLike,
    successes: ArrayLike,
    trials: ArrayLike,
    names: Sequence[str],
    firth: bool = False,
    max_iter: int = 200,
    tol: float = 1e-10,
) -> LogitFit:
    """Ajuste logit(p) = Xβ sur des données binomiales agrégées par IRLS.

    Avec ``firth=True``, la vraisemblance est pénalisée par la loi a priori de
    Jeffreys (correction de Firth), ce qui garantit des estimations finies en
    cas de séparation (cellules à 0 % ou 100 % de victoires).
    """
    x = np.asarray(design, dtype=float)
    k = np.asarray(successes, dtype=float)
    n = np.asarray(trials, dtype=float)
    if x.ndim != 2 or x.shape[0] != k.shape[0] or k.shape != n.shape:
        raise ValueError("dimensions incohérentes")
    if x.shape[1] != len(names):
        raise ValueError("nombre de noms différent du nombre de colonnes")
    beta = np.zeros(x.shape[1])
    converged = False
    iteration = 0
    for iteration in range(1, max_iter + 1):  # noqa: B007 (lu apres la boucle)
        eta = x @ beta
        mu = special.expit(eta)
        w = np.clip(n * mu * (1.0 - mu), 1e-12, None)
        info = x.T @ (w[:, None] * x)
        try:
            info_inv = np.linalg.inv(info)
        except np.linalg.LinAlgError:
            info_inv = np.linalg.pinv(info)
        residual = k - n * mu
        if firth:
            # Diagonale de la matrice chapeau W^½ X (XᵀWX)⁻¹ Xᵀ W^½.
            h = np.einsum("ij,jk,ik->i", x, info_inv, x) * w
            residual = residual + h * (0.5 - mu)
        step = info_inv @ (x.T @ residual)
        # Pas amorti si la vraisemblance (pénalisée) se dégrade.
        beta_new = beta + step
        if np.max(np.abs(step)) < tol:
            beta = beta_new
            converged = True
            break
        if not firth:
            old = _binomial_loglik(k, n, eta)
            scale = 1.0
            while _binomial_loglik(k, n, x @ beta_new) < old - 1e-12 and scale > 1e-4:
                scale /= 2.0
                beta_new = beta + scale * step
        beta = beta_new
    eta = x @ beta
    mu = special.expit(eta)
    w = np.clip(n * mu * (1.0 - mu), 1e-12, None)
    info = x.T @ (w[:, None] * x)
    try:
        cov = np.linalg.inv(info)
    except np.linalg.LinAlgError:
        cov = np.linalg.pinv(info)
    loglik = _binomial_loglik(k, n, eta)
    deviance = 2.0 * (_saturated_loglik(k, n) - loglik)
    return LogitFit(
        names=tuple(names),
        params=beta,
        cov=cov,
        loglik=loglik,
        deviance=deviance,
        n_obs=int(n.sum()),
        converged=converged,
        iterations=iteration,
        firth=firth,
    )


def likelihood_ratio_test(full: LogitFit, reduced: LogitFit) -> tuple[float, int, float]:
    """Test du rapport de vraisemblance entre modèles emboîtés : (stat, ddl, p)."""
    stat = max(0.0, 2.0 * (full.loglik - reduced.loglik))
    df = len(full.names) - len(reduced.names)
    if df <= 0:
        raise ValueError("le modèle complet doit avoir plus de paramètres")
    return stat, df, float(stats.chi2.sf(stat, df))


def percentile_interval(samples: ArrayLike, confidence: float = 0.95) -> tuple[float, float]:
    """Intervalle percentile d'un échantillon bootstrap (valeurs finies seulement)."""
    values = np.asarray(samples, dtype=float)
    values = values[np.isfinite(values)]
    if values.size == 0:
        return float("nan"), float("nan")
    alpha = (1.0 - confidence) / 2.0
    lo, hi = np.quantile(values, [alpha, 1.0 - alpha])
    return float(lo), float(hi)
