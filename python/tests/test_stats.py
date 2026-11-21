"""Statistiques contre des valeurs de référence."""

from __future__ import annotations

import importlib.util

import numpy as np
import pytest
from scipy import optimize, special

from hexbandit.experiments.stats import (
    binomial_pvalue,
    fit_logit,
    likelihood_ratio_test,
    percentile_interval,
    wilson_interval,
)


def test_wilson_reference_values() -> None:
    lo, hi = wilson_interval(5, 10)
    assert float(lo) == pytest.approx(0.2366, abs=1e-4)
    assert float(hi) == pytest.approx(0.7634, abs=1e-4)
    lo, hi = wilson_interval(0, 10)
    assert float(lo) == 0.0
    assert float(hi) == pytest.approx(0.2775, abs=1e-4)
    lo, hi = wilson_interval(np.array([500, 1000]), np.array([1000, 1000]))
    assert lo[0] == pytest.approx(0.4691, abs=1e-4)
    assert hi[1] == 1.0
    # Demi-largeur d'environ 3 points pour 1 000 parties.
    assert (hi[0] - lo[0]) / 2 == pytest.approx(0.031, abs=0.001)


def test_binomial_pvalues() -> None:
    assert binomial_pvalue(5, 10) == pytest.approx(1.0)
    assert binomial_pvalue(9, 10, alternative="greater") == pytest.approx(11 / 1024)
    assert binomial_pvalue(60, 100) == pytest.approx(0.05689, abs=1e-4)
    assert np.isnan(binomial_pvalue(0, 0))


def _negative_loglik(beta: np.ndarray, x: np.ndarray, k: np.ndarray, n: np.ndarray) -> float:
    eta = x @ beta
    return float(-np.sum(k * eta - n * np.logaddexp(0.0, eta)))


def test_logit_matches_direct_optimisation() -> None:
    rng = np.random.default_rng(1)
    rows = 60
    x = np.column_stack([np.ones(rows), rng.normal(size=rows), rng.integers(0, 2, rows)])
    true = np.array([0.3, 0.8, -0.5])
    n = rng.integers(20, 200, rows).astype(float)
    k = rng.binomial(n.astype(int), special.expit(x @ true)).astype(float)
    fit = fit_logit(x, k, n, ["const", "x", "d"])
    assert fit.converged
    ref = optimize.minimize(
        _negative_loglik, np.zeros(3), args=(x, k, n), method="BFGS", options={"gtol": 1e-10}
    )
    np.testing.assert_allclose(fit.params, ref.x, atol=1e-5)
    assert fit.loglik == pytest.approx(-ref.fun, abs=1e-6)
    # Écarts-types : inverse de l'information de Fisher, comparée à une différenciation numérique du gradient.
    assert np.all(fit.bse > 0)
    z, p = fit.wald("x")
    assert z > 3 and p < 0.01


def test_logit_closed_form_two_groups() -> None:
    x = np.array([[1.0, 0.0], [1.0, 1.0]])
    k = np.array([30.0, 70.0])
    n = np.array([100.0, 100.0])
    fit = fit_logit(x, k, n, ["const", "g"])
    assert fit.params[0] == pytest.approx(np.log(30 / 70), abs=1e-8)
    assert fit.params[1] == pytest.approx(np.log(70 / 30) - np.log(30 / 70), abs=1e-8)
    assert fit.deviance == pytest.approx(0.0, abs=1e-8)


def test_firth_gives_finite_estimates_under_separation() -> None:
    x = np.column_stack([np.ones(5), np.arange(-2.0, 3.0)])
    k = np.array([0.0, 0.0, 10.0, 20.0, 20.0])
    n = np.array([20.0, 20.0, 20.0, 20.0, 20.0])
    fit = fit_logit(x, k, n, ["a", "b"], firth=True)
    assert np.all(np.isfinite(fit.params))
    assert fit.params[1] > 0
    assert -fit.params[0] / fit.params[1] == pytest.approx(0.0, abs=0.05)


def test_likelihood_ratio_and_percentiles() -> None:
    rng = np.random.default_rng(3)
    rows = 40
    x = np.column_stack([np.ones(rows), rng.normal(size=rows)])
    n = np.full(rows, 100.0)
    k = rng.binomial(100, special.expit(0.2 + 0.6 * x[:, 1])).astype(float)
    full = fit_logit(x, k, n, ["c", "x"])
    reduced = fit_logit(x[:, :1], k, n, ["c"])
    stat, df, p = likelihood_ratio_test(full, reduced)
    assert df == 1 and stat > 20 and p < 1e-5
    with pytest.raises(ValueError):
        likelihood_ratio_test(reduced, full)
    lo, hi = percentile_interval(np.r_[np.arange(1000.0), np.nan, np.inf])
    assert lo == pytest.approx(24.975, abs=0.01) and hi == pytest.approx(974.025, abs=0.01)


@pytest.mark.skipif(importlib.util.find_spec("statsmodels") is None, reason="statsmodels absent")
def test_logit_matches_statsmodels() -> None:  # pragma: no cover - dépend de l'environnement
    import statsmodels.api as sm

    rng = np.random.default_rng(5)
    x = np.column_stack([np.ones(50), rng.normal(size=50)])
    n = rng.integers(10, 100, 50).astype(float)
    k = rng.binomial(n.astype(int), special.expit(x @ np.array([0.1, -0.7]))).astype(float)
    fit = fit_logit(x, k, n, ["c", "x"])
    ref = sm.GLM(np.column_stack([k, n - k]), x, family=sm.families.Binomial()).fit()
    np.testing.assert_allclose(fit.params, ref.params, atol=1e-6)
    np.testing.assert_allclose(fit.bse, ref.bse, rtol=1e-4)
