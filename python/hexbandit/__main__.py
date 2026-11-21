"""Point d'entrée : ``python -m hexbandit``.

L'environnement est vérifié avant d'importer la ligne de commande, pour
afficher un message clair plutôt qu'une trace ``ModuleNotFoundError``.
"""

from __future__ import annotations

import sys

from hexbandit._environment import environment_problems, explain


def _run() -> int:
    problems = environment_problems()
    if problems:
        print(explain(problems), file=sys.stderr)
        return 3
    from hexbandit.cli.main import main

    return main()


sys.exit(_run())
