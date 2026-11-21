"""Visionneuse web des parties.

Page HTML autonome (sans dépendance externe) : rejeu coup par coup, cartes
de chaleur des visites et des valeurs Q, Q̃ et Q*, table des candidats,
informations du solveur et de la réutilisation de l'arbre. Elle lit le format
``hexbandit/game-record`` : enregistrements intégrés par :func:`build_viewer` ou
ouverts depuis le navigateur.
"""

from __future__ import annotations

import json
from importlib import resources
from pathlib import Path
from typing import Any

PLACEHOLDER = "/*__HEXBANDIT_RECORDS__*/[]"


def template() -> str:
    """Code HTML de la visionneuse (sans partie intégrée)."""
    return resources.files("hexbandit.viz.web").joinpath("viewer.html").read_text(encoding="utf-8")


def build_viewer(records: list[dict[str, Any]], path: Path) -> Path:
    """Écrit une page contenant la visionneuse et les enregistrements donnés."""
    payload = json.dumps(records, ensure_ascii=False, separators=(",", ":"))
    # « </ » fermerait la balise <script> : on l'échappe (JSON reste valide).
    payload = payload.replace("</", "<\\/")
    html = template()
    if PLACEHOLDER not in html:
        raise RuntimeError("gabarit de la visionneuse altéré : marqueur des parties absent")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(html.replace(PLACEHOLDER, payload, 1), encoding="utf-8")
    return path
