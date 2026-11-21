"""Rendu du plateau de Hex avec Matplotlib.

Géométrie : hexagones « pointe en haut » ; la case (ligne r, colonne c) a pour
centre x = c + r/2, y = -r·√3/2, ce qui donne le losange classique (chaque
ligne décalée d'une demi-case vers la droite). Les bords Ouest et Est sont
bleus (Bleu les relie), les bords Nord et Sud rouges.

Ce module ne dépend que du format d'enregistrement, jamais du moteur.
"""

from __future__ import annotations

import math
from collections.abc import Mapping
from dataclasses import dataclass

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
import numpy as np
from matplotlib.axes import Axes
from matplotlib.colors import LinearSegmentedColormap, Normalize
from matplotlib.figure import Figure
from matplotlib.patches import Polygon

from hexbandit.domain.hexboard import HexBoard, parse_cell, split_chameleon_move
from hexbandit.domain.types import Color

SURFACE = "#fcfcfb"
INK = "#0b0b0b"
INK_2 = "#52514e"
EMPTY_FILL = "#f0efec"
CELL_EDGE = "#c3c2b7"
PLAYER_COLORS: dict[Color, str] = {"blue": "#2a78d6", "red": "#e34948"}
HEAT = LinearSegmentedColormap.from_list(
    "visits", ["#eef7f3", "#8fd6b9", "#1baf7a", "#0e7a52", "#064a31"]
)

RADIUS = 1.0 / math.sqrt(3.0)
ROW_STEP = math.sqrt(3.0) / 2.0
# Sommets d'un hexagone « pointe en haut » : haut, haut-droit, bas-droit, bas, bas-gauche, haut-gauche.
VERTEX_ANGLES = [90.0, 30.0, -30.0, -90.0, -150.0, 150.0]


def cell_center(row: int, col: int) -> tuple[float, float]:
    """Centre de la case (ligne, colonne)."""
    return col + 0.5 * row, -row * ROW_STEP


def hex_vertices(row: int, col: int, scale: float = 1.0) -> list[tuple[float, float]]:
    """Sommets de l'hexagone de la case."""
    x, y = cell_center(row, col)
    r = RADIUS * scale
    return [
        (x + r * math.cos(math.radians(a)), y + r * math.sin(math.radians(a)))
        for a in VERTEX_ANGLES
    ]


@dataclass
class BoardStyle:
    """Options de rendu."""

    show_coordinates: bool = True
    show_move_numbers: bool = True
    highlight_path: bool = True
    title: str | None = None


def _border_segments(
    size: int,
) -> dict[Color, list[tuple[tuple[float, float], tuple[float, float]]]]:
    """Arêtes extérieures du losange : bords bleus (Ouest/Est) et rouges (Nord/Sud)."""
    segments: dict[Color, list[tuple[tuple[float, float], tuple[float, float]]]] = {
        "blue": [],
        "red": [],
    }
    last = size - 1
    for i in range(size):
        top, upper_right, lower_right, bottom, lower_left, upper_left = hex_vertices(0, i)
        segments["red"] += [(upper_left, top), (top, upper_right)]
        top, upper_right, lower_right, bottom, lower_left, upper_left = hex_vertices(last, i)
        segments["red"] += [(lower_left, bottom), (bottom, lower_right)]
        top, upper_right, lower_right, bottom, lower_left, upper_left = hex_vertices(i, 0)
        segments["blue"] += [(upper_left, lower_left)]
        if i < last:
            segments["blue"] += [(lower_left, bottom)]
        top, upper_right, lower_right, bottom, lower_left, upper_left = hex_vertices(i, last)
        segments["blue"] += [(upper_right, lower_right)]
        if i > 0:
            segments["blue"] += [(top, upper_right)]
    return segments


def draw_board(
    ax: Axes,
    board: HexBoard,
    style: BoardStyle | None = None,
    heat: Mapping[tuple[int, int], float] | None = None,
    heat_label: str = "",
) -> None:
    """Dessine ``board`` dans ``ax`` (pierres, bords, coordonnées, chemin gagnant)."""
    style = style or BoardStyle()
    size = board.size
    ax.set_facecolor(SURFACE)
    ax.set_aspect("equal")
    ax.axis("off")
    # Bords colorés, dessinés sous les cases (seule la moitié extérieure dépasse).
    for color, segments in _border_segments(size).items():
        for (x0, y0), (x1, y1) in segments:
            ax.plot(
                [x0, x1],
                [y0, y1],
                color=PLAYER_COLORS[color],
                linewidth=7.0 * 11 / max(size, 5),
                solid_capstyle="round",
                zorder=1,
            )
    winner = board.winner()
    path_cells = (
        board.winning_path(winner) if (winner is not None and style.highlight_path) else None
    )
    path = set(path_cells or [])
    norm = None
    if heat:
        values = np.array(list(heat.values()), dtype=float)
        norm = Normalize(
            vmin=0.0, vmax=float(values.max()) if values.size and values.max() > 0 else 1.0
        )
    font = max(4.5, 9.0 * 11 / size)
    for row in range(size):
        for col in range(size):
            owner = board.stones.get((row, col))
            face: str | tuple[float, float, float, float]
            if owner is not None:
                face = PLAYER_COLORS[owner]
            elif heat is not None and norm is not None and (row, col) in heat:
                face = HEAT(norm(heat[(row, col)]))
            else:
                face = EMPTY_FILL
            edge = INK if (row, col) in path else CELL_EDGE
            ax.add_patch(
                Polygon(
                    hex_vertices(row, col, 0.98),
                    closed=True,
                    facecolor=face,
                    edgecolor=edge,
                    linewidth=2.2 if (row, col) in path else 0.8,
                    zorder=3 if (row, col) in path else 2,
                )
            )
            x, y = cell_center(row, col)
            if owner is not None and style.show_move_numbers and (row, col) in board.order:
                ax.text(
                    x,
                    y,
                    str(board.order[(row, col)]),
                    ha="center",
                    va="center",
                    fontsize=font * 0.8,
                    color="#ffffff",
                    zorder=4,
                )
    if path_cells:
        xs, ys = zip(*(cell_center(r, c) for r, c in path_cells), strict=True)
        ax.plot(xs, ys, color="#ffffff", linewidth=1.0, alpha=0.9, zorder=4)
    if style.show_coordinates:
        for i in range(size):
            x, y = cell_center(0, i)
            ax.text(
                x, y + 0.85, chr(ord("a") + i), ha="center", va="center", fontsize=font, color=INK_2
            )
            x, y = cell_center(size - 1, i)
            ax.text(
                x, y - 0.85, chr(ord("a") + i), ha="center", va="center", fontsize=font, color=INK_2
            )
            x, y = cell_center(i, 0)
            ax.text(x - 0.95, y, str(i + 1), ha="center", va="center", fontsize=font, color=INK_2)
            x, y = cell_center(i, size - 1)
            ax.text(x + 0.95, y, str(i + 1), ha="center", va="center", fontsize=font, color=INK_2)
    x_min, _ = cell_center(0, 0)
    x_max, _ = cell_center(size - 1, size - 1)
    ax.set_xlim(x_min - 1.6, x_max + 1.6)
    ax.set_ylim(-(size - 1) * ROW_STEP - 1.5, 1.5)
    if style.title:
        ax.set_title(style.title, fontsize=10, color=INK, loc="left")
    if heat is not None and norm is not None:
        mappable = plt.cm.ScalarMappable(norm=norm, cmap=HEAT)
        bar = ax.figure.colorbar(mappable, ax=ax, shrink=0.6, pad=0.01)
        bar.set_label(heat_label)
        bar.outline.set_visible(False)


def board_figure(
    board: HexBoard,
    style: BoardStyle | None = None,
    heat: Mapping[tuple[int, int], float] | None = None,
    heat_label: str = "",
) -> Figure:
    """Figure Matplotlib contenant le plateau."""
    size = board.size
    width = 1.6 + 0.62 * size * 1.5
    fig, ax = plt.subplots(
        figsize=(width + (1.2 if heat is not None else 0.0), 0.62 * size * 1.0 + 1.6)
    )
    fig.patch.set_facecolor(SURFACE)
    draw_board(ax, board, style, heat, heat_label)
    return fig


def heat_from_candidates(
    size: int, candidates: list[tuple[str, float]], combine: str = "max"
) -> dict[tuple[int, int], float]:
    """Valeurs par case à partir de paires (coup, valeur). Les coups de
    Chameleon (``c4b``, ``c4r``) sont regroupés par case : somme (visites) ou
    maximum (valeurs) des deux couleurs."""
    heat: dict[tuple[int, int], float] = {}
    for move, value in candidates:
        text = move.strip().lower()
        if text == "swap":
            continue
        if text[-1:] in ("b", "r") or ":" in text:
            text, _ = split_chameleon_move(text)
        cell = parse_cell(text, size)
        if cell in heat:
            heat[cell] = (
                heat[cell] + float(value) if combine == "sum" else max(heat[cell], float(value))
            )
        else:
            heat[cell] = float(value)
    return heat
