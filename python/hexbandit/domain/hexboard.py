"""Modèle de plateau minimal pour l'affichage et le rejeu.

Aucune simulation ici : ce modèle rejoue des coups enregistrés et
calcule le chemin gagnant à mettre en évidence. La géométrie est celle
du moteur : la case (ligne r, colonne c) a pour voisines (r±1, c), (r, c±1),
(r+1, c-1) et (r-1, c+1). Bleu relie Ouest et Est, Rouge relie Nord et Sud.

Chameleon : les pierres ont une couleur indépendante du joueur (coup
``c4b`` ou ``c4r``) ; Bleu gagne avec une chaîne d'une même couleur
d'Ouest en Est, Rouge du Nord au Sud.
"""

from __future__ import annotations

from collections import deque
from collections.abc import Iterable, Iterator
from dataclasses import dataclass

from hexbandit.domain.errors import GameRuleError
from hexbandit.domain.types import Color, other_color

NEIGHBOR_OFFSETS: tuple[tuple[int, int], ...] = ((-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0))


def parse_cell(text: str, size: int) -> tuple[int, int]:
    """Lit un coup ``c4`` : renvoie (ligne, colonne) à partir de 0."""
    cleaned = text.strip().lower()
    if len(cleaned) < 2 or not cleaned[0].isalpha() or not cleaned[1:].isdigit():
        raise GameRuleError(f"coup illisible : {text!r}")
    col = ord(cleaned[0]) - ord("a")
    row = int(cleaned[1:]) - 1
    if not (0 <= row < size and 0 <= col < size):
        raise GameRuleError(f"coup hors du plateau : {text!r}")
    return row, col


def format_cell(row: int, col: int) -> str:
    """Écrit un coup en notation ``c4``."""
    return f"{chr(ord('a') + col)}{row + 1}"


def split_chameleon_move(text: str) -> tuple[str, Color]:
    """Case et couleur d'un coup de Chameleon (``c4b``, ``c4r``, ``c4:blue``)."""
    cleaned = text.strip().lower()
    if ":" in cleaned:
        cell, _, color = cleaned.partition(":")
    else:
        cell, color = cleaned[:-1], cleaned[-1:]
    names: dict[str, Color] = {
        "b": "blue",
        "blue": "blue",
        "bleu": "blue",
        "r": "red",
        "red": "red",
        "rouge": "red",
    }
    if color not in names or not cell:
        raise GameRuleError(f"coup de Chameleon illisible : {text!r}")
    return cell, names[color]


@dataclass
class HexBoard:
    """Plateau de Hex (ou de Chameleon) rejouable, pour l'affichage."""

    size: int
    first_player: Color = "blue"
    chameleon: bool = False

    def __post_init__(self) -> None:
        if not 2 <= self.size <= 19:
            raise GameRuleError(f"taille {self.size} hors de [2, 19]")
        self.stones: dict[tuple[int, int], Color] = {}
        self.order: dict[tuple[int, int], int] = {}
        self.to_move: Color = self.first_player
        self.last_mover: Color | None = None

    def neighbors(self, row: int, col: int) -> Iterator[tuple[int, int]]:
        """Cases voisines dans le plateau."""
        for dr, dc in NEIGHBOR_OFFSETS:
            r, c = row + dr, col + dc
            if 0 <= r < self.size and 0 <= c < self.size:
                yield r, c

    def play(self, move: str, color: Color | None = None) -> None:
        """Pose une pierre (``swap`` applique la règle du gâteau) ; ``color``
        est le joueur qui joue (au Chameleon, la couleur de la pierre est dans le coup)."""
        player = color or self.to_move
        if move.strip().lower() == "swap":
            if len(self.stones) != 1:
                raise GameRuleError("l'échange n'est possible qu'après le premier coup")
            (((r, c), owner),) = self.stones.items()
            # Hex : la pierre change de camp ; Chameleon : elle garde sa couleur.
            self.stones = {(c, r): owner if self.chameleon else other_color(owner)}
            self.order = {(c, r): 1}
            self.to_move = other_color(player)
            self.last_mover = player
            return
        stone = player
        if self.chameleon:
            move, stone = split_chameleon_move(move)
        cell = parse_cell(move, self.size)
        if cell in self.stones:
            raise GameRuleError(f"case occupée : {move}")
        self.stones[cell] = stone
        self.order[cell] = len(self.order) + 1
        self.to_move = other_color(player)
        self.last_mover = player

    def play_all(self, moves: Iterable[str]) -> None:
        """Joue une suite de coups."""
        for move in moves:
            self.play(move)

    def _starts(self, color: Color, cell: tuple[int, int]) -> bool:
        return cell[1] == 0 if color == "blue" else cell[0] == 0

    def _goals(self, color: Color, cell: tuple[int, int]) -> bool:
        last = self.size - 1
        return cell[1] == last if color == "blue" else cell[0] == last

    def winning_path(self, color: Color) -> list[tuple[int, int]] | None:
        """Plus court chemin (en pierres) reliant les bords de ``color`` ; au
        Chameleon, plus court parmi les chaînes des deux couleurs de pierres."""
        if not self.chameleon:
            return self._path(color, color)
        paths = [p for p in (self._path(color, "blue"), self._path(color, "red")) if p]
        return min(paths, key=len) if paths else None

    def _path(self, color: Color, stone: Color) -> list[tuple[int, int]] | None:
        """Plus court chemin de pierres ``stone`` reliant les bords de ``color``."""
        starts = [
            cell
            for cell, owner in self.stones.items()
            if owner == stone and self._starts(color, cell)
        ]
        previous: dict[tuple[int, int], tuple[int, int] | None] = {cell: None for cell in starts}
        queue = deque(sorted(starts))
        while queue:
            cell = queue.popleft()
            if self._goals(color, cell):
                path = [cell]
                while (prev := previous[path[-1]]) is not None:
                    path.append(prev)
                return path[::-1]
            for nb in self.neighbors(*cell):
                if nb not in previous and self.stones.get(nb) == stone:
                    previous[nb] = cell
                    queue.append(nb)
        return None

    def winner(self) -> Color | None:
        """Camp qui relie ses bords, s'il existe (Chameleon : si les deux
        directions sont reliées, l'auteur du dernier coup)."""
        colors: tuple[Color, Color] = ("blue", "red")
        connected = [c for c in colors if self.winning_path(c) is not None]
        if len(connected) == 2 and self.chameleon:
            return self.last_mover or other_color(self.to_move)
        return connected[0] if connected else None

    def ascii(self) -> str:
        """Rendu texte (même disposition que le rendu ASCII du moteur)."""
        letters = " ".join(chr(ord("a") + c) for c in range(self.size))
        lines = [f"    {letters}"]
        for row in range(self.size):
            cells = " ".join(
                {"blue": "B", "red": "R"}.get(self.stones.get((row, col), ""), ".")
                for col in range(self.size)
            )
            lines.append(f"{' ' * row}{row + 1:>2}  {cells}  {row + 1}")
        lines.append(f"{' ' * (self.size + 3)}{letters}")
        return "\n".join(lines)
