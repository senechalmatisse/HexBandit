"""Campagnes avec le moteur factice : plan, coût, reprise, analyse."""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest

from hexbandit.domain.errors import ConfigError
from hexbandit.engine.fake import FakeEngine, default_model
from hexbandit.experiments.analysis import analyse, cell_table
from hexbandit.experiments.campaign import CampaignConfig, CampaignRunner, estimate_cost
from hexbandit.experiments.repository import FileResultsRepository, build_metadata


def test_plan_expansion_and_seeds(campaign_file: Path) -> None:
    config = CampaignConfig.load(campaign_file)
    cells = config.cells()
    assert len(cells) == 2 * 1 * 2 * 3
    ids = {c.cell_id for c in cells}
    assert "rave_B16_N07_k-2" in ids
    cell = next(c for c in cells if c.cell_id == "uct_B16_N05_k+2")
    assert (cell.budget_first, cell.budget_second, cell.ratio) == (64, 16, 4.0)
    assert len({c.master_seed for c in cells}) == len(cells)
    # Les graines ne dépendent que de la graine maître et de l'identifiant.
    again = {c.cell_id: c.master_seed for c in CampaignConfig.load(campaign_file).cells()}
    assert all(again[c.cell_id] == c.master_seed for c in cells)
    spec = config.match_spec(cell, 20, 20)
    assert spec.offset == 20 and spec.a.simulations == 64 and spec.b.simulations == 16
    assert spec.seating == "a_first_alternate_colors"


@pytest.mark.parametrize(
    "patch",
    [
        ("games_per_cell = 40", "games_per_cell = 41"),
        ('algorithms = ["uct", "rave"]', 'algorithms = ["uct", "alphazero"]'),
        ("base_budgets = [16]", "base_budgets = [3]"),
        ("sizes = [5, 7]", "sizes = [5, 25]"),
        ('name = "test"', 'name = "test"\nunknown_field = 1'),
    ],
)
def test_invalid_campaigns_are_rejected(campaign_file: Path, patch: tuple[str, str]) -> None:
    campaign_file.write_text(campaign_file.read_text().replace(*patch), encoding="utf-8")
    with pytest.raises(ConfigError):
        CampaignConfig.load(campaign_file)


def test_reductions_and_cost(campaign_file: Path) -> None:
    text = (
        campaign_file.read_text()
        + "\n[[reductions]]\nsizes = [7]\ngames_per_cell = 10\nratio_exponents = [0]\n"
    )
    campaign_file.write_text(text, encoding="utf-8")
    config = CampaignConfig.load(campaign_file)
    n7 = [c for c in config.cells() if c.size == 7]
    assert len(n7) == 2 and all(c.games == 10 and c.ratio_exponent == 0 for c in n7)
    estimate = estimate_cost(config)
    expected = sum(
        c.games * 0.6 * c.size**2 / 2 * (c.budget_first + c.budget_second) for c in config.cells()
    )
    assert estimate.expected_simulations == pytest.approx(expected)
    assert estimate.max_simulations > estimate.expected_simulations
    assert "N= 5" in estimate.describe()


def test_cost_ceiling_refuses(campaign_file: Path, tmp_path: Path) -> None:
    text = campaign_file.read_text().replace(
        "max_cost_simulations = 1e12", "max_cost_simulations = 10"
    )
    campaign_file.write_text(text, encoding="utf-8")
    config = CampaignConfig.load(campaign_file)
    runner = CampaignRunner(config, FakeEngine(), FileResultsRepository(tmp_path / "r"), {})
    with pytest.raises(ConfigError, match="plafond"):
        runner.run()
    runner.run(force=True)


def test_incremental_save_and_resume(campaign_file: Path, tmp_path: Path) -> None:
    config = CampaignConfig.load(campaign_file)
    repository = FileResultsRepository(tmp_path / "results")
    engine = FakeEngine()
    metadata = build_metadata(config, engine.version())
    first = CampaignRunner(config, engine, repository, metadata).run(time_limit=-1.0)
    assert first.stopped_by_time_limit and first.chunks_done == 0
    # Exécution partielle simulée : on enregistre seulement trois tranches.
    partial = CampaignRunner(config, engine, repository, metadata)
    for cell in config.cells()[:3]:
        spec = config.match_spec(cell, 0, 20)
        from hexbandit.experiments.campaign import ChunkResult, rows_from_records

        rows = rows_from_records(config, cell, engine.play_match(spec, 1), metadata)
        repository.save(ChunkResult(cell=cell, offset=0, rows=rows))
    summary = partial.run()
    assert summary.complete
    assert summary.chunks_skipped == 3
    assert summary.chunks_done == 2 * len(config.cells()) - 3
    games = repository.load_all()
    assert len(games) == 40 * len(config.cells())
    assert set(games["config_hash"]) == {config.config_hash()}
    # Relancer ne rejoue rien et donne exactement les mêmes données.
    calls = engine.calls
    again = CampaignRunner(config, engine, repository, metadata).run()
    assert again.chunks_done == 0 and engine.calls == calls
    # Couleurs alternées : A commence, Bleu dans la moitié des parties.
    per_cell = games.groupby("cell_id")["first_color"].apply(lambda s: (s == "blue").mean())
    assert np.allclose(per_cell, 0.5)


def test_analysis_recovers_known_equilibrium(tmp_path: Path, campaign_file: Path) -> None:
    text = (
        campaign_file.read_text()
        .replace("games_per_cell = 40", "games_per_cell = 600")
        .replace("chunk_size = 20", "chunk_size = 600")
        .replace("ratio_exponents = [-2, 0, 2]", "ratio_exponents = [-4, -2, 0, 2, 4]")
    )
    campaign_file.write_text(text, encoding="utf-8")
    config = CampaignConfig.load(campaign_file)
    repository = FileResultsRepository(tmp_path / "results")
    CampaignRunner(config, FakeEngine(), repository, {}).run()
    games = repository.load_all()
    cells = cell_table(games)
    assert cells["n"].eq(600).all()
    result = analyse(games, n_boot=300, seed=1)
    for estimate in result.estimates:
        intercept = 0.2 + 0.08 * estimate.size + (0.3 if estimate.algorithm == "rave" else 0.0)
        true = -intercept / 0.45
        lo, hi = estimate.log2_rstar_ci
        assert lo - 0.15 <= true <= hi + 0.15, (estimate.algorithm, estimate.size, true, lo, hi)
        assert estimate.slope == pytest.approx(0.45, abs=0.12)
        assert estimate.p_first_r1 == pytest.approx(
            default_model(estimate.size, 0.0, estimate.algorithm), abs=0.07
        )
    assert result.tests["H1"]["groups_positive"] == len(result.estimates)
    assert result.tests["H2"]["groups_rstar_below_1"] == len(result.estimates)
    assert result.tests["H4"]["p_value"] < 0.05
    assert 0.4 < result.tests["color_bias"]["blue_rate"] < 0.6


def test_two_slope_model_captures_asymmetry(tmp_path: Path, campaign_file: Path) -> None:
    """Courbe asymétrique (pente 2 pour r < 1, 0,5 pour r > 1) : le modèle à deux
    pentes retrouve les pentes et r*, le modèle linéaire est rejeté."""

    def asymmetric(size: int, x: float, algorithm: str) -> float:
        eta = 0.6 + (2.0 * x if x < 0 else 0.5 * x)
        return float(1.0 / (1.0 + np.exp(-eta)))

    text = (
        campaign_file.read_text()
        .replace("games_per_cell = 40", "games_per_cell = 1000")
        .replace("chunk_size = 20", "chunk_size = 1000")
        .replace(
            "ratio_exponents = [-2, 0, 2]", "ratio_exponents = [-4, -3, -2, -1, 0, 1, 2, 3, 4]"
        )
        .replace('algorithms = ["uct", "rave"]', 'algorithms = ["uct"]')
        .replace("sizes = [5, 7]", "sizes = [9]")
    )
    campaign_file.write_text(text, encoding="utf-8")
    config = CampaignConfig.load(campaign_file)
    repository = FileResultsRepository(tmp_path / "results")
    CampaignRunner(config, FakeEngine(model=asymmetric), repository, {}).run()
    (estimate,) = analyse(repository.load_all(), n_boot=200, seed=3).estimates
    assert estimate.slope_minus == pytest.approx(2.0, abs=0.35)
    assert estimate.slope_plus == pytest.approx(0.5, abs=0.15)
    assert estimate.asymmetry_p_value < 1e-6
    assert estimate.lack_of_fit_p_linear < 0.01 < estimate.lack_of_fit_p_two_slopes
    lo, hi = estimate.log2_rstar_ci
    assert lo - 0.05 <= -0.3 <= hi + 0.05
