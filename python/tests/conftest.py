"""Configuration commune des tests."""

from __future__ import annotations

from pathlib import Path

import pytest

from hexbandit.engine.native import native_available

requires_native = pytest.mark.skipif(
    not native_available(), reason="bibliothèque native absente (scripts/hexlXtc01.sh)"
)

CAMPAIGN_TOML = """
[campaign]
name = "test"
master_seed = 123
games_per_cell = 40
chunk_size = 20
threads = 1
max_cost_simulations = 1e12

[plan]
sizes = [5, 7]
ratio_exponents = [-2, 0, 2]
base_budgets = [16]
algorithms = ["uct", "rave"]

[players.uct]
algorithm = "uct"

[players.rave]
algorithm = "rave"
beta = { schedule = "mse", bias = 0.05 }
"""


@pytest.fixture
def campaign_file(tmp_path: Path) -> Path:
    """Petit fichier de campagne valide."""
    path = tmp_path / "campagne.toml"
    path.write_text(CAMPAIGN_TOML, encoding="utf-8")
    return path
