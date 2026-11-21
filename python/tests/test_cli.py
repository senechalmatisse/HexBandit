"""Ligne de commande : analyse des joueurs, campagne factice et rapport."""

from __future__ import annotations

import argparse
from pathlib import Path

import pytest

from hexbandit.cli.main import main, parse_player


def test_parse_player() -> None:
    p = parse_player("rave:2000,beta=manual,k=500,c=0.1,rollout=uniform")
    assert p.algorithm == "rave" and p.simulations == 2000
    assert p.beta is not None and p.beta.schedule == "manual" and p.beta.k == 500
    assert p.exploration == 0.1 and p.rollout == "uniform"
    assert parse_player("uct:10,time=50").time_ms == 50
    assert parse_player("random").algorithm == "random"
    for bad in ("uct", "uct:10,foo=1", "uct:0"):
        with pytest.raises(argparse.ArgumentTypeError):
            parse_player(bad)


def test_campaign_estimate_run_and_report(
    campaign_file: Path, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    assert main(["estimate", str(campaign_file)]) == 0
    results = tmp_path / "res"
    assert main(["run-campaign", str(campaign_file), "--results", str(results), "--fake"]) == 0
    assert (results / "metadata.json").exists()
    assert main(["run-campaign", str(campaign_file), "--results", str(results), "--fake"]) == 0
    out = capsys.readouterr().out
    assert "0 tranches jouées" in out
    assert main(["report", str(results), "--bootstrap", "50"]) == 0
    assert (results / "rapport" / "resultats.md").exists()
    assert (results / "rapport" / "figures" / "g1_p_premier_vs_ratio.png").exists()
    # Une configuration modifiée n'est pas reprise silencieusement.
    campaign_file.write_text(
        campaign_file.read_text().replace("master_seed = 123", "master_seed = 124")
    )
    assert main(["run-campaign", str(campaign_file), "--results", str(results), "--fake"]) == 4


def test_version_and_environment_diagnostics(
    capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    from hexbandit import _environment

    with pytest.raises(SystemExit) as stop:
        main(["--version"])
    assert stop.value.code == 0
    out = capsys.readouterr().out
    assert out.startswith("hexbandit ") and "Python " in out and "moteur natif" in out

    assert _environment.environment_problems() == []
    assert _environment.missing_modules(("json", "module_hexbandit_absent")) == [
        "module_hexbandit_absent"
    ]
    monkeypatch.setattr(_environment, "missing_modules", lambda: ["module_hexbandit_absent"])
    problems = _environment.environment_problems()
    assert len(problems) == 1 and "module_hexbandit_absent" in problems[0]
    message = _environment.explain(problems)
    assert "scripts/hexlXtc03.sh" in message and ".venv/bin/activate" in message


def test_parse_player_v2_options() -> None:
    p = parse_player("rave:500,solver,reuse,grave=40,trees=3,prior=hex,prior.dead=50,final=secure")
    assert (p.solver, p.reuse_tree, p.grave_ref, p.trees) == (True, True, 40, 3)
    assert p.prior is not None and p.prior.kind == "hex" and p.prior.dead == 50
    assert p.final_move == "secure"
    assert parse_player("uct:10,prior=even:20").to_engine()["prior"] == {
        "kind": "even",
        "confidence": 20,
    }
    assert parse_player("rave:10,solver=0").solver is False
    for bad in (
        "uct:10,grave=5",
        "rave:10,prior=even",
        "rave:10,trees=0",
        "rave:10,solver=peut-etre",
        "rave:10,secure_a=1",
        "rave:abc",
    ):
        with pytest.raises(argparse.ArgumentTypeError):
            parse_player(bad)
