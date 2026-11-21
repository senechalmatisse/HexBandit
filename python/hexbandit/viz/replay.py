"""Rejeu d'une partie enregistrée : image finale, suite d'images,
animation GIF ou MP4, carte de chaleur d'un coup.

:class:`MatplotlibRenderer` implémente le port :class:`hexbandit.domain.ports.Renderer`.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Literal

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
from matplotlib import animation
from matplotlib.artist import Artist

from hexbandit.domain.errors import RecordFormatError
from hexbandit.domain.hexboard import HexBoard
from hexbandit.domain.types import GameRecord
from hexbandit.viz.board import (
    BoardStyle,
    board_figure,
    draw_board,
    heat_from_candidates,
)

COLOR_NAMES = {"blue": "Bleu", "red": "Rouge", "draw": "nul"}
GAME_NAMES = {"hex": "Hex", "reverse_hex": "Reverse Hex", "chameleon": "Chameleon"}


def load_record(path: str | Path) -> GameRecord:
    """Lit un enregistrement JSON."""
    try:
        data = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise RecordFormatError(f"lecture impossible de {path} : {exc}") from exc
    return GameRecord.from_json(data)


def board_at(record: GameRecord, ply: int | None = None) -> HexBoard:
    """Plateau après ``ply`` coups (tous les coups si ``None``), ouverture comprise."""
    if record.kind not in ("hex", "reverse_hex", "chameleon"):
        raise RecordFormatError(
            f"visualisation disponible pour le Hex et ses variantes uniquement (jeu : {record.kind})"
        )
    board = HexBoard(record.size, record.first_player, chameleon=record.kind == "chameleon")
    board.play_all(record.opening)
    moves = record.moves if ply is None else record.moves[:ply]
    for move in moves:
        board.play(move.move, move.player)
    return board


def _player_label(record: GameRecord, color: str) -> str:
    config = record.players.get(color, {})
    return str(config.get("label", config.get("algorithm", "?")))


def describe(record: GameRecord, ply: int | None = None) -> str:
    """Titre : joueurs, coup courant, résultat."""
    blue, red = _player_label(record, "blue"), _player_label(record, "red")
    game = GAME_NAMES.get(record.kind, record.kind)
    head = f"{game} {record.size}×{record.size} · Bleu : {blue} · Rouge : {red}"
    if ply is None or ply >= record.num_moves:
        winner = COLOR_NAMES.get(record.winner or "", "?")
        return f"{head}\n{record.num_moves} coups · vainqueur : {winner}"
    return f"{head}\ncoup {ply}/{record.num_moves}"


class MatplotlibRenderer:
    """Rendu Matplotlib des parties enregistrées."""

    def __init__(self, style: BoardStyle | None = None) -> None:
        self.style = style or BoardStyle()

    def _style(self, title: str) -> BoardStyle:
        return BoardStyle(
            show_coordinates=self.style.show_coordinates,
            show_move_numbers=self.style.show_move_numbers,
            highlight_path=self.style.highlight_path,
            title=title,
        )

    def render_final(self, record: GameRecord, path: Path) -> Path:
        """Image de la position finale (chemin gagnant mis en évidence)."""
        fig = board_figure(board_at(record), self._style(describe(record)))
        path.parent.mkdir(parents=True, exist_ok=True)
        fig.savefig(path, dpi=150, bbox_inches="tight")
        plt.close(fig)
        return path

    def render_frames(self, record: GameRecord, directory: Path) -> list[Path]:
        """Une image PNG par coup (``coup_000.png`` = position initiale)."""
        directory.mkdir(parents=True, exist_ok=True)
        paths = []
        for ply in range(record.num_moves + 1):
            fig = board_figure(board_at(record, ply), self._style(describe(record, ply)))
            path = directory / f"coup_{ply:03d}.png"
            fig.savefig(path, dpi=110, bbox_inches="tight")
            plt.close(fig)
            paths.append(path)
        return paths

    def render_animation(self, record: GameRecord, path: Path, fps: float = 4.0) -> Path:
        """Animation de la partie : GIF (Pillow) ou MP4 (ffmpeg) selon l'extension."""
        fmt: Literal["gif", "mp4"] = "mp4" if path.suffix.lower() == ".mp4" else "gif"
        fig = board_figure(board_at(record, 0), self._style(describe(record, 0)))
        ax = fig.axes[0]

        def update(ply: int) -> list[Artist]:
            ax.clear()
            draw_board(ax, board_at(record, ply), self._style(describe(record, ply)))
            return []

        rate = max(1, round(fps))
        frames = list(range(record.num_moves + 1)) + [record.num_moves] * (2 * rate)
        anim = animation.FuncAnimation(fig, update, frames=frames, blit=False)
        writer: animation.AbstractMovieWriter
        if fmt == "gif":
            writer = animation.PillowWriter(fps=rate)
        else:
            # libx264 en yuv420p exige des dimensions paires.
            writer = animation.FFMpegWriter(
                fps=rate,
                codec="libx264",
                extra_args=["-pix_fmt", "yuv420p", "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2"],
            )
        path.parent.mkdir(parents=True, exist_ok=True)
        anim.save(str(path), writer=writer, dpi=100)
        plt.close(fig)
        return path

    def render_heatmap(
        self,
        record: GameRecord,
        ply: int,
        path: Path,
        metric: Literal["visits", "value", "amaf"] = "visits",
    ) -> Path:
        """Carte de chaleur des statistiques MCTS du coup ``ply`` (rapport « full » requis)."""
        if not 1 <= ply <= record.num_moves:
            raise RecordFormatError(f"coup {ply} hors de [1, {record.num_moves}]")
        move = record.moves[ply - 1]
        if move.report is None or not move.report.candidates:
            raise RecordFormatError(
                "ce coup n'a pas de rapport complet (rejouer avec report='full')"
            )
        report = move.report
        if metric == "visits":
            pairs = [(c.move, c.n / max(1, report.simulations)) for c in report.candidates]
            label = "part des simulations N(s,a) / N(s)"
        elif metric == "amaf":
            pairs = [(c.move, c.q_amaf) for c in report.candidates if c.n_amaf > 0]
            label = "valeur AMAF Q̃(s,a)"
        else:
            pairs = [
                (c.move, c.value) for c in report.candidates if c.value is not None and c.n > 0
            ]
            label = "valeur de l'action (Q ou Q*)"
        heat = heat_from_candidates(
            record.size, pairs, combine="sum" if metric == "visits" else "max"
        )
        board = board_at(record, ply - 1)
        title = (
            f"{describe(record, ply - 1).splitlines()[0]}\n"
            f"coup {ply} ({COLOR_NAMES[move.player]}) : {move.move} · "
            f"{report.simulations} simulations · valeur racine {report.root_value:.2f}"
        )
        fig = board_figure(board, self._style(title), heat=heat, heat_label=label)
        path.parent.mkdir(parents=True, exist_ok=True)
        fig.savefig(path, dpi=150, bbox_inches="tight")
        plt.close(fig)
        return path
