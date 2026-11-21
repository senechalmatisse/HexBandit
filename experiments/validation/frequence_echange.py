"""Fréquence de l'échange avec la règle du gâteau.

Deux IA identiques à 256 simulations par coup ; parties de graines 1000 à
1000 + n - 1, Bleu commence. Compte les parties où le second joueur échange,
et le taux de victoire du premier joueur (celui qui pose la première pierre).

Depuis ``python/`` : ``python ../experiments/validation/frequence_echange.py 2``
(argument : nombre de threads) ; sortie recopiée dans ``frequence_echange.txt``.
"""

from __future__ import annotations

import sys
from collections import Counter

from hexbandit.domain.types import GameConfig, GameSpec, PlayerSpec
from hexbandit.engine.native import NativeEngine

PLANS = [
    ("rave", 5, 400),
    ("rave", 7, 400),
    ("rave", 9, 200),
    ("rave", 14, 100),
    ("uct", 5, 400),
    ("uct", 7, 400),
    ("uct", 9, 200),
    ("uct", 14, 100),
]


def main(threads: int) -> None:
    engine = NativeEngine()
    for algorithm, size, n in PLANS:
        player = PlayerSpec(
            algorithm=algorithm, simulations=256, exploration=0.25 if algorithm == "uct" else None
        )
        configs = [
            GameConfig(
                game=GameSpec(kind="hex", size=size, swap=True),
                first_player="blue",
                blue=player,
                red=player,
                seed=1000 + i,
                report="none",
            )
            for i in range(n)
        ]
        records = engine.play_batch(configs, threads=threads)
        swaps = first_won = first_won_after_swap = 0
        openings: Counter[str] = Counter()
        for record in records:
            moves = [m.move for m in record.moves]
            swapped = len(moves) > 1 and moves[1] == "swap"
            won = record.winner == "blue"
            swaps += swapped
            first_won += won
            first_won_after_swap += won and swapped
            openings[moves[0]] += 1
        print(
            f"{algorithm} {size}x{size} : echange {swaps}/{n} ({swaps / n:.0%}) ; "
            f"premier joueur gagnant {first_won / n:.3f} ; apres echange {first_won_after_swap}/{swaps} ; "
            f"{len(openings)} ouvertures differentes, les plus jouees {openings.most_common(3)}",
            flush=True,
        )


if __name__ == "__main__":
    main(int(sys.argv[1]) if len(sys.argv) > 1 else 2)
