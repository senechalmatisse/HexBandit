"""Vérification de l'environnement Python avant d'importer le reste du paquet.

Ce module n'importe que la bibliothèque standard : il doit pouvoir expliquer
ce qui manque même quand numpy, pandas ou pydantic sont absents.
"""

from __future__ import annotations

import importlib.util
import sys
from collections.abc import Sequence

MIN_PYTHON = (3, 11)
"""Plus ancienne version de Python prise en charge (``tomllib``, ``datetime.UTC``)."""

REQUIRED_MODULES = ("numpy", "pandas", "scipy", "matplotlib", "pydantic")
"""Modules importés par la ligne de commande et les campagnes."""

SETUP_HINT = (
    "Créez l'environnement virtuel du projet, depuis la racine du dépôt :\n"
    "  bash scripts/hexlXtc03.sh\n"
    "  source .venv/bin/activate\n"
    "puis relancez la commande (python désigne alors l'interpréteur de .venv)."
)


def python_label() -> str:
    """Version et chemin de l'interpréteur courant."""
    version = ".".join(str(part) for part in sys.version_info[:3])
    return f"Python {version} ({sys.executable})"


def missing_modules(modules: Sequence[str] = REQUIRED_MODULES) -> list[str]:
    """Modules introuvables par l'interpréteur courant, sans les importer."""
    return [name for name in modules if importlib.util.find_spec(name) is None]


def environment_problems() -> list[str]:
    """Liste des problèmes bloquants (vide si l'environnement convient)."""
    problems: list[str] = []
    if sys.version_info < MIN_PYTHON:
        wanted = ".".join(str(part) for part in MIN_PYTHON)
        problems.append(f"{python_label()} est trop ancien : Python >= {wanted} requis.")
    missing = missing_modules()
    if missing:
        problems.append(f"modules absents pour {python_label()} : {', '.join(missing)}.")
    return problems


def explain(problems: Sequence[str]) -> str:
    """Message d'erreur destiné à l'utilisateur."""
    lines = ["hexbandit : environnement Python incomplet."]
    lines.extend(f"  - {problem}" for problem in problems)
    lines.append(SETUP_HINT)
    return "\n".join(lines)
