"""Plateau d'affichage, enregistrements et rendu."""

from __future__ import annotations

from pathlib import Path

import pandas as pd
import pytest

from hexbandit.domain.errors import GameRuleError, RecordFormatError
from hexbandit.domain.hexboard import HexBoard, format_cell, parse_cell
from hexbandit.domain.types import GameConfig, GameRecord, GameSpec, PlayerSpec
from hexbandit.engine.fake import FakeEngine
from hexbandit.experiments.analysis import analyse
from hexbandit.experiments.plots import plot_all
from hexbandit.experiments.report import write_report
from hexbandit.viz.replay import MatplotlibRenderer, board_at


def test_notation_round_trip() -> None:
    for size in (2, 9, 19):
        for row in range(size):
            for col in range(size):
                assert parse_cell(format_cell(row, col), size) == (row, col)
    for bad in ("", "a0", "z1", "11", "c20"):
        with pytest.raises(GameRuleError):
            parse_cell(bad, 19)


def test_board_winner_path_and_swap() -> None:
    board = HexBoard(3)
    board.play_all(["a1", "a3", "b1", "b3", "c1"])
    assert board.winner() == "blue"
    assert board.winning_path("blue") == [(0, 0), (0, 1), (0, 2)]
    with pytest.raises(GameRuleError):
        board.play("a1")
    swap = HexBoard(5)
    swap.play("b1")
    swap.play("swap")
    assert swap.stones == {(1, 0): "red"} and swap.to_move == "blue"
    red = HexBoard(3, "red")
    red.play_all(["c1", "a1", "b2", "a2", "a3"])
    assert red.winner() == "red"
    assert "R" in red.ascii() and red.ascii().count("\n") == 4


def test_record_parsing_rejects_unknown_formats() -> None:
    with pytest.raises(RecordFormatError):
        GameRecord.from_json({"format": "other", "format_version": 1})
    with pytest.raises(RecordFormatError):
        GameRecord.from_json({"format": "hexbandit/game-record", "format_version": 99})
    with pytest.raises(RecordFormatError):
        GameRecord.from_json({"format": "hexbandit/game-record", "format_version": 1, "moves": []})


def _fake_record() -> GameRecord:
    config = GameConfig(
        game=GameSpec(size=5),
        blue=PlayerSpec(algorithm="uct", simulations=10),
        red=PlayerSpec(algorithm="uct", simulations=10),
        seed=4,
    )
    return FakeEngine().play_game(config)


def test_renderer_outputs(tmp_path: Path) -> None:
    record = _fake_record()
    renderer = MatplotlibRenderer()
    final = renderer.render_final(record, tmp_path / "final.png")
    assert final.stat().st_size > 1000
    frames = renderer.render_frames(record, tmp_path / "frames")
    assert len(frames) == record.num_moves + 1
    gif = renderer.render_animation(record, tmp_path / "game.gif", fps=8)
    assert gif.read_bytes()[:3] == b"GIF"
    board = board_at(record, 3)
    assert len(board.stones) == 3
    with pytest.raises(RecordFormatError):
        renderer.render_heatmap(record, 1, tmp_path / "heat.png")


def test_plots_and_report_from_fake_campaign(tmp_path: Path) -> None:
    engine = FakeEngine()
    rows = []
    for size in (5, 7, 9):
        for k in (-2, 0, 2):
            for i in range(60):
                first = "blue" if i % 2 == 0 else "red"
                won = (i * 7 + size + k) % 10 < 5 + k + size // 3
                rows.append(
                    {
                        "algorithm": "uct" if i % 3 else "rave",
                        "base_budget": 16,
                        "size": size,
                        "ratio_exponent": k,
                        "first_won": int(won),
                        "winner": first if won else ("red" if first == "blue" else "blue"),
                        "num_moves": 10,
                        "duration_ms": 1.0,
                    }
                )
    games = pd.DataFrame(rows)
    result = analyse(games, n_boot=100, seed=2)
    figures = plot_all(result, tmp_path / "figures")
    assert set(figures) == {"G1", "G2", "G3", "G4", "G5"}
    assert all(p.stat().st_size > 5000 for p in figures.values())
    report = write_report(
        result, {"campaign": "fake", "engine": engine.version()}, figures, tmp_path / "r.md"
    )
    text = report.read_text(encoding="utf-8")
    assert "H1" in text and "figures/g3_ratio_equilibre.png" in text and "Annexe" in text


def test_mp4_animation_has_even_dimensions(tmp_path: Path) -> None:
    import shutil

    if shutil.which("ffmpeg") is None:
        pytest.skip("ffmpeg absent")
    path = MatplotlibRenderer().render_animation(_fake_record(), tmp_path / "game.mp4", fps=8)
    assert path.stat().st_size > 1000


def test_chameleon_board_and_heatmap(tmp_path: Path) -> None:
    from hexbandit.domain.hexboard import split_chameleon_move
    from hexbandit.viz.board import heat_from_candidates

    board = HexBoard(3, chameleon=True)
    board.play_all(["a2r", "a1b", "b2r", "c1b"])
    assert board.winner() is None
    board.play("c2r")
    # Chaîne rouge d'Ouest en Est : victoire de Bleu (Horz).
    assert board.winner() == "blue"
    assert board.winning_path("blue") == [(1, 0), (1, 1), (1, 2)]
    assert board.stones[(1, 2)] == "red" and board.to_move == "red"
    assert split_chameleon_move("C4:Rouge") == ("c4", "red")
    with pytest.raises(GameRuleError):
        split_chameleon_move("c4x")
    # Deux directions d'un coup (2 × 2, diagonale a2-b1) : l'auteur gagne.
    both = HexBoard(2, "red", chameleon=True)
    both.play_all(["a2b", "b1b"])
    assert both.winner() == "blue"
    heat = heat_from_candidates(3, [("a1b", 0.25), ("a1r", 0.5), ("b2r", 0.1)], combine="sum")
    assert heat == {(0, 0): 0.75, (1, 1): 0.1}
    swap = HexBoard(5, chameleon=True)
    swap.play("b1r")
    swap.play("swap")
    assert swap.stones == {(1, 0): "red"} and swap.to_move == "blue"


def test_web_viewer_embeds_records(tmp_path: Path) -> None:
    import json

    from hexbandit.cli.main import main
    from hexbandit.viz.web import PLACEHOLDER, build_viewer, template

    assert PLACEHOLDER in template()
    record = _fake_record().raw or {
        "format": "hexbandit/game-record",
        "moves": [],
        "note": "</script><b>",
    }
    record = {**record, "note": "</script><b>"}
    page = build_viewer([record], tmp_path / "page.html").read_text(encoding="utf-8")
    assert PLACEHOLDER not in page and "</script><b>" not in page
    start = page.index("const EMBEDDED = ") + len("const EMBEDDED = ")
    embedded = json.loads(page[start : page.index(";\n", start)].replace("<\\/", "</"))
    assert embedded[0]["note"] == "</script><b>"
    source = tmp_path / "partie.json"
    source.write_text(json.dumps({"records": [record, record]}), encoding="utf-8")
    assert main(["view", str(source)]) == 0
    assert (tmp_path / "partie.html").stat().st_size > len(template())
