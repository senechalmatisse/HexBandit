"""Tests du moteur natif à travers le port (ignorés sans bibliothèque compilée)."""

from __future__ import annotations

import threading
import time
from pathlib import Path

import pytest

from hexbandit import _native
from hexbandit.domain.errors import Cancelled, ConfigError, GameRuleError
from hexbandit.domain.types import (
    GameConfig,
    GameSpec,
    MatchSpec,
    PlayerSpec,
    PriorSpec,
    SearchRequest,
)
from hexbandit.engine.native import NativeEngine
from hexbandit.viz.replay import MatplotlibRenderer, board_at

from .conftest import requires_native

pytestmark = [requires_native, pytest.mark.native]

UCT = PlayerSpec(algorithm="uct", simulations=300)
RAVE = PlayerSpec(algorithm="rave", simulations=300)


def test_game_is_deterministic_and_well_formed() -> None:
    engine = NativeEngine()
    config = GameConfig(game=GameSpec(size=7), blue=RAVE, red=UCT, seed=11, report="full")
    a, b = engine.play_game(config), engine.play_game(config)
    assert [m.move for m in a.moves] == [m.move for m in b.moves]
    assert a.winner in ("blue", "red")
    assert a.moves[-1].player == a.winner
    assert a.moves[0].report is not None and a.moves[0].report.simulations == 300
    assert len(a.moves[0].report.candidates) == 49
    assert a.players["red"]["algorithm"] == "uct"


def test_typed_errors() -> None:
    engine = NativeEngine()
    with pytest.raises(GameRuleError):
        engine.play_game(GameConfig(game=GameSpec(size=5), blue=UCT, red=UCT, opening=("a1", "a1")))
    with pytest.raises(ConfigError):
        _native.play_game(
            {
                "players": {
                    "blue": {"algorithm": "uct"},
                    "red": {"algorithm": "uct", "simulations": 1},
                }
            }
        )


def test_match_is_independent_of_threads() -> None:
    engine = NativeEngine()
    spec = MatchSpec(
        game=GameSpec(size=6),
        a=UCT,
        b=RAVE,
        n_games=8,
        master_seed=3,
        seating="a_first_alternate_colors",
    )
    one = engine.play_match(spec, 1)
    two = engine.play_match(spec, 2)
    assert [[m.move for m in r.moves] for r in one] == [[m.move for m in r.moves] for r in two]
    assert [r.a_color for r in one] == ["blue", "red"] * 4
    assert all(r.first_player == r.a_color for r in one)
    batch = engine.play_batch(
        [GameConfig(game=GameSpec(size=4), blue=UCT, red=UCT, seed=s) for s in range(4)], 2
    )
    assert len(batch) == 4


def test_search_finds_unique_win() -> None:
    result = NativeEngine().search(
        SearchRequest(
            game=GameSpec(size=5),
            moves=("a1", "c2", "b1", "c3", "d1", "c4", "e1", "c5"),
            player=PlayerSpec(algorithm="rave", simulations=2000),
        )
    )
    assert result.best_move == "c1" and result.player == "blue"
    assert result.report.candidates[0].move == "c1"


def test_cancellation_from_another_thread() -> None:
    spec = MatchSpec(
        game=GameSpec(size=11),
        a=PlayerSpec(algorithm="uct", simulations=200_000),
        b=UCT,
        n_games=4,
        master_seed=1,
    )
    errors: list[BaseException] = []

    def run() -> None:
        try:
            NativeEngine().play_match(spec, 1)
        except BaseException as exc:
            errors.append(exc)

    thread = threading.Thread(target=run)
    thread.start()
    time.sleep(0.5)
    _native.cancel()
    thread.join(timeout=30)
    assert not thread.is_alive()
    assert errors and isinstance(errors[0], Cancelled)


def test_heatmap_from_native_record(tmp_path: Path) -> None:
    record = NativeEngine().play_game(
        GameConfig(game=GameSpec(size=5), blue=RAVE, red=UCT, seed=2, report="full")
    )
    path = MatplotlibRenderer().render_heatmap(record, 1, tmp_path / "heat.png", metric="visits")
    assert path.stat().st_size > 1000


def test_quarto_variant() -> None:
    engine = NativeEngine()
    game = GameSpec(kind="quarto")
    assert game.size == 4
    record = engine.play_game(
        GameConfig(game=game, blue=UCT, red=PlayerSpec(algorithm="random"), seed=1)
    )
    assert record.moves[0].move.startswith("/")
    assert record.winner in ("blue", "red", "draw")
    with pytest.raises(ConfigError, match="AmafGame"):
        engine.play_game(GameConfig(game=game, blue=RAVE, red=UCT))
    with pytest.raises(ValueError):
        GameSpec(kind="quarto", size=5)


def test_v2_options_round_trip_through_the_engine() -> None:
    engine = NativeEngine()
    v2 = PlayerSpec(
        algorithm="rave",
        simulations=400,
        solver=True,
        reuse_tree=True,
        trees=2,
        prior=PriorSpec(kind="hex"),
        grave_ref=20,
    )
    config = GameConfig(game=GameSpec(size=5), blue=v2, red=UCT, seed=4, report="full")
    a, b = engine.play_game(config), engine.play_game(config)
    assert [m.move for m in a.moves] == [m.move for m in b.moves]
    blue_reports = [m.report for m in a.moves if m.player == "blue" and m.report is not None]
    assert any(r.reused_visits > 0 for r in blue_reports)
    assert any(r.proven == "win" for r in blue_reports)
    assert a.players["blue"]["trees"] == 2 and a.players["blue"]["solver"] is True
    result = engine.search(
        SearchRequest(
            game=GameSpec(size=5),
            moves=("a1", "c2", "b1", "c3", "d1", "c4", "e1", "c5"),
            player=PlayerSpec(algorithm="uct", simulations=3000, exploration=0.25, solver=True),
        )
    )
    assert result.best_move == "c1" and result.report.proven == "win"
    assert result.report.candidates[0].proven == "win"
    with pytest.raises(ConfigError):
        engine.play_game(
            GameConfig(game=GameSpec(kind="reverse_hex", size=5), blue=v2, red=UCT, seed=1)
        )


def test_chameleon_game_and_rendering(tmp_path: Path) -> None:
    engine = NativeEngine()
    config = GameConfig(
        game=GameSpec(kind="chameleon", size=5), blue=RAVE, red=UCT, seed=3, report="full"
    )
    record = engine.play_game(config)
    assert record.kind == "chameleon" and record.winner in ("blue", "red")
    assert all(m.move[-1] in "br" for m in record.moves)
    renderer = MatplotlibRenderer()
    assert renderer.render_final(record, tmp_path / "c.png").stat().st_size > 1000
    heat = renderer.render_heatmap(record, 1, tmp_path / "h.png", metric="visits")
    assert heat.stat().st_size > 1000
    board = board_at(record)
    assert board.winner() == record.winner
