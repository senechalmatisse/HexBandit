"""Statistiques : Bradley-Terry et Elo, modèle mixte, splines
monotones, puissance, plan adaptatif et tournoi (moteur factice)."""

from __future__ import annotations

import math
from pathlib import Path

import numpy as np
import pandas as pd
import pytest
from scipy import special

from hexbandit.cli.main import main
from hexbandit.domain.errors import ConfigError
from hexbandit.engine.fake import FakeEngine
from hexbandit.experiments.adaptive import AdaptiveConfig, AdaptiveRunner, Posterior, results_frame
from hexbandit.experiments.analysis import cell_table
from hexbandit.experiments.campaign import CampaignConfig, CampaignRunner
from hexbandit.experiments.elo import ELO_PER_LOGIT, budget_ladders, fit_bradley_terry
from hexbandit.experiments.models import (
    fit_mixed_model,
    fit_monotone_splines,
    ispline_basis,
    power_analysis,
)
from hexbandit.experiments.repository import FileResultsRepository
from hexbandit.experiments.tournament import TournamentConfig, run_tournament


def test_bradley_terry_recovers_known_strengths() -> None:
    rng = np.random.default_rng(1)
    theta = {"a": 0.0, "b": 0.8, "c": 1.5, "d": 1.9}
    gamma = 0.35
    first, second, won = [], [], []
    for x in theta:
        for y in theta:
            if x == y:
                continue
            p = special.expit(theta[x] - theta[y] + gamma)
            for w in rng.random(3000) < p:
                first.append(x)
                second.append(y)
                won.append(int(w))
    fit = fit_bradley_terry(first, second, won, players=list(theta), n_boot=50)
    np.testing.assert_allclose(fit.strength, list(theta.values()), atol=0.08)
    assert fit.advantage == pytest.approx(gamma, abs=0.05)
    assert fit.elo[3] == pytest.approx(1.9 * ELO_PER_LOGIT, abs=15)
    assert fit.deviance < 30 and fit.bootstrap.shape == (50, 5)


def _fake_campaign(
    tmp_path: Path, campaign_file: Path, model: object | None = None
) -> pd.DataFrame:
    text = (
        campaign_file.read_text()
        .replace("games_per_cell = 40", "games_per_cell = 400")
        .replace("chunk_size = 20", "chunk_size = 400")
        .replace("ratio_exponents = [-2, 0, 2]", "ratio_exponents = [-4, -2, -1, 0, 1, 2, 4]")
        .replace("base_budgets = [16]", "base_budgets = [16, 64]")
    )
    campaign_file.write_text(text, encoding="utf-8")
    config = CampaignConfig.load(campaign_file)
    repository = FileResultsRepository(tmp_path / "results")
    engine = FakeEngine() if model is None else FakeEngine(model=model)  # type: ignore[arg-type]
    CampaignRunner(config, engine, repository, {}).run()
    return repository.load_all()


def test_budget_ladder_matches_the_fake_model(tmp_path: Path, campaign_file: Path) -> None:
    # Modèle factice : logit P = a(N) + 0,45·log2(B_premier/B_second), soit
    # θ(B) = 0,45·log2 B et γ = a(N) : 78 Elo par doublement.
    games = _fake_campaign(tmp_path, campaign_file)
    ladders = budget_ladders(games, n_boot=30)
    assert {(lad.algorithm, lad.size) for lad in ladders} == {
        ("uct", 5),
        ("uct", 7),
        ("rave", 5),
        ("rave", 7),
    }
    for ladder in ladders:
        intercept = 0.2 + 0.08 * ladder.size + (0.3 if ladder.algorithm == "rave" else 0.0)
        assert ladder.fit.advantage == pytest.approx(intercept, abs=0.15)
        gains = ladder.per_doubling()["elo_gain"]
        assert gains.mean() == pytest.approx(0.45 * ELO_PER_LOGIT, abs=25)
        assert ladder.equilibrium_log2_ratio(16) == pytest.approx(-intercept / 0.45, abs=0.5)


def _cells(x: np.ndarray, eta: np.ndarray, n: int, rng: np.random.Generator) -> pd.DataFrame:
    rows = []
    for group in range(4):
        wins = rng.binomial(n, special.expit(eta + rng.normal(0, 0.0, len(x))))
        for xi, w in zip(x, wins, strict=True):
            rows.append(
                {
                    "algorithm": "uct",
                    "base_budget": 16,
                    "size": 5 + 2 * group,
                    "ratio_exponent": xi,
                    "wins": float(w),
                    "n": float(n),
                }
            )
    return pd.DataFrame(rows)


def test_mixed_model_detects_overdispersion() -> None:
    rng = np.random.default_rng(3)
    x = np.arange(-4.0, 5.0)
    eta = 0.6 + np.where(x < 0, 1.5 * x, 0.6 * x)
    rows = []
    for group in range(6):
        noise = rng.normal(0.0, 0.35, len(x))
        wins = rng.binomial(800, special.expit(eta + noise))
        rows += [
            {
                "algorithm": "rave",
                "base_budget": 16,
                "size": 5 + group,
                "ratio_exponent": xi,
                "wins": float(w),
                "n": 800.0,
            }
            for xi, w in zip(x, wins, strict=True)
        ]
    noisy = fit_mixed_model(pd.DataFrame(rows), n_draws=500)
    assert 0.2 < noisy.sigma < 0.6 and noisy.lrt_p_value < 1e-3
    clean = fit_mixed_model(_cells(x, eta, 800, rng), n_draws=500)
    assert clean.sigma < noisy.sigma and clean.lrt_p_value > 0.01
    true = -0.6 / 1.5
    for estimate in clean.groups:
        lo, hi = estimate.log2_rstar_ci
        assert lo - 0.1 <= true <= hi + 0.1


def test_monotone_splines_are_increasing_and_find_rstar() -> None:
    basis = ispline_basis(np.linspace(-4, 4, 50), np.array([-2.0, 0.0, 2.0]), -4.0, 4.0)
    assert np.all(np.diff(basis, axis=0) >= -1e-12)
    assert basis[0].max() < 1e-9 and np.allclose(basis[-1], 1.0)
    rng = np.random.default_rng(5)
    x = np.arange(-4.0, 5.0)
    cells = _cells(x, 0.8 + np.where(x < 0, 2.0 * x, 0.5 * x), 1000, rng)
    for fit in fit_monotone_splines(cells, n_boot=40):
        lo, hi = fit.log2_rstar_ci
        assert lo - 0.1 <= -0.4 <= hi + 0.1
        assert np.all(np.diff(fit.eta(np.linspace(-4, 4, 40))) >= -1e-9)


def test_power_analysis_scales_with_variance(tmp_path: Path, campaign_file: Path) -> None:
    games = _fake_campaign(tmp_path, campaign_file)
    power = power_analysis(cell_table(games), rstar_halfwidth=0.2)
    assert len(power) == 8
    finite = power[np.isfinite(power["log2_rstar_se"])]
    expected = np.ceil((1.959964 * finite["log2_rstar_se"] / 0.2) ** 2 * finite["games_per_cell"])
    np.testing.assert_allclose(finite["games_per_cell_for_rstar"], expected, rtol=1e-6)
    assert (power["games_for_p_r1"] > 200).all()


ADAPTIVE = """
[campaign]
name = "adaptatif"
master_seed = 11
threads = 1

[adaptive]
sizes = [9]
base_budgets = [64]
algorithms = ["uct"]
batch = 40
max_games = 1600
target_width = 0.4

[players.uct]
algorithm = "uct"
"""


def _steep(size: int, x: float, algorithm: str) -> float:
    return float(special.expit(1.6 * (x + 0.7)))


def test_adaptive_design_finds_rstar_with_few_games(tmp_path: Path) -> None:
    path = tmp_path / "adaptatif.toml"
    path.write_text(ADAPTIVE, encoding="utf-8")
    config = AdaptiveConfig.load(path)
    runner = AdaptiveRunner(config, FakeEngine(model=_steep), tmp_path / "res")
    (result,) = runner.run()
    summary = result.summary(config.adaptive)
    assert result.complete and summary["width"] < 0.4
    assert summary["log2_rstar_low"] <= -0.7 <= summary["log2_rstar_high"]
    # Les rapports joués se concentrent près de l'équilibre.
    xs = np.array([s.x for s in result.steps])
    assert np.median(np.abs(xs + 0.7)) < 1.0
    assert result.games < 1600
    # Reprise : rien n'est rejoué ; configuration modifiée : refus.
    engine = FakeEngine(model=_steep)
    AdaptiveRunner(config, engine, tmp_path / "res").run()
    assert engine.calls == 0
    changed = AdaptiveConfig.load(path).model_copy(
        update={"adaptive": config.adaptive.model_copy(update={"batch": 20})}
    )
    with pytest.raises(ConfigError):
        AdaptiveRunner(changed, FakeEngine(model=_steep), tmp_path / "res").run()
    assert (
        len(results_frame(config, AdaptiveRunner(config, engine, tmp_path / "res").results())) == 1
    )


def test_posterior_concentrates_where_data_are() -> None:
    post = Posterior.prior(-5, 3)
    for x in (-1.0, -0.5, 0.0, 0.5):
        p = float(special.expit(2.0 * (x + 0.3)))
        post.update(x, round(400 * p), 400)
    _, median, lo, hi = post.alpha_summary()
    assert lo < -0.3 < hi and abs(median + 0.3) < 0.1
    design = post.best_design(np.linspace(-5, 3, 161))
    assert abs(design - median) < 1.0


TOURNAMENT = """
[tournament]
name = "test"
size = 5
games_per_pair = 40
master_seed = 3
threads = 1

[players.faible]
algorithm = "uct"
simulations = 16

[players.moyen]
algorithm = "uct"
simulations = 64

[players.fort]
algorithm = "uct"
simulations = 256
"""


def test_tournament_ranks_budgets(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    path = tmp_path / "tournoi.toml"
    path.write_text(TOURNAMENT, encoding="utf-8")
    result = run_tournament(TournamentConfig.load(path), FakeEngine(), n_boot=20)
    standings = result.standings()
    assert list(standings["player"]) == ["fort", "moyen", "faible"]
    assert len(result.games) == 120 and standings["games"].eq(80).all()
    assert main(["tournament", str(path), "--fake", "--out", str(tmp_path / "t")]) == 0
    assert (tmp_path / "t" / "classement.csv").exists()
    assert "classement" in capsys.readouterr().out


def test_adaptive_and_extended_report_through_the_cli(
    tmp_path: Path, campaign_file: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    path = tmp_path / "adaptatif.toml"
    path.write_text(ADAPTIVE.replace("max_games = 1600", "max_games = 400"), encoding="utf-8")
    code = main(["adaptive", str(path), "--results", str(tmp_path / "a"), "--fake"])
    assert code in (0, 5)
    text = (tmp_path / "a" / "rapport" / "resultats.md").read_text(encoding="utf-8")
    assert "Plan adaptatif" in text and "uct" in text
    _fake_campaign(tmp_path, campaign_file)
    assert main(["report", str(tmp_path / "results"), "--bootstrap", "100"]) == 0
    report = (tmp_path / "results" / "rapport" / "resultats.md").read_text(encoding="utf-8")
    for needle in ("Échelle Elo", "Modèle mixte", "Analyse de puissance", "g6_elo", "g7_rstar"):
        assert needle in report, needle
    assert math.isfinite(
        pd.read_csv(tmp_path / "results" / "rapport" / "rstar_modeles.csv")[
            "log2_rstar_mixed"
        ].iloc[0]
    )
