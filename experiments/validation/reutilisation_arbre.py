"""Part du budget apportée par la réutilisation de l'arbre.

MC-RAVE à 1 000 simulations avec réutilisation (Bleu, qui commence) contre
MC-RAVE à 1 000 simulations sans (Rouge), 20 parties de graines 500 à 519 en
9 × 9 et en 14 × 14 ; pour chaque coup de Bleu après le premier, visites
réutilisées divisées par le budget.

Depuis ``python/`` : ``python ../experiments/validation/reutilisation_arbre.py`` ;
sortie recopiée dans ``reutilisation_arbre.txt``.
"""

from __future__ import annotations

import statistics

from hexbandit.domain.types import GameConfig, GameSpec, PlayerSpec
from hexbandit.engine.native import NativeEngine


def main() -> None:
    engine = NativeEngine()
    reuse = PlayerSpec(algorithm="rave", simulations=1000, reuse_tree=True)
    reference = PlayerSpec(algorithm="rave", simulations=1000)
    for size in (9, 14):
        configs = [
            GameConfig(
                game=GameSpec(kind="hex", size=size),
                first_player="blue",
                blue=reuse,
                red=reference,
                seed=500 + i,
                report="summary",
            )
            for i in range(20)
        ]
        fractions = [
            (m.report.reused_visits or 0) / 1000
            for record in engine.play_batch(configs, threads=1)
            for m in record.moves
            if m.player == "blue" and m.report is not None and m.ply > 1
        ]
        q1, median, q3 = statistics.quantiles(fractions, n=4)
        print(
            f"{size}x{size} : {len(fractions)} coups ; visites reutilisees / budget : "
            f"moyenne {statistics.mean(fractions):.3f}, quartiles {q1:.3f} {median:.3f} {q3:.3f}, "
            f"maximum {max(fractions):.3f}"
        )


if __name__ == "__main__":
    main()
