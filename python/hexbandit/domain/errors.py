"""Exceptions typées de hexbandit.

Les erreurs renvoyées par le moteur natif portent une catégorie (``kind``)
convertie en l'une de ces exceptions.
"""

from __future__ import annotations


class HexbanditError(Exception):
    """Erreur de base de hexbandit."""


class ConfigError(HexbanditError):
    """Configuration ou requête invalide."""


class GameRuleError(HexbanditError):
    """Règle du jeu violée (coup illégal, position impossible)."""


class SearchFailed(HexbanditError):
    """Erreur du moteur de recherche."""


class Cancelled(HexbanditError):
    """Calcul interrompu (Ctrl-C)."""


class EngineInternalError(HexbanditError):
    """Erreur interne du moteur (panique interceptée)."""


class NativeLibraryNotFound(HexbanditError):
    """Bibliothèque native introuvable : lancer ``scripts/hexlXtc01.sh``."""


class RecordFormatError(HexbanditError):
    """Enregistrement de partie illisible ou de version inconnue."""


ERROR_KINDS: dict[str, type[HexbanditError]] = {
    "config": ConfigError,
    "game": GameRuleError,
    "search": SearchFailed,
    "cancelled": Cancelled,
    "internal": EngineInternalError,
}


def error_from_kind(kind: str, message: str) -> HexbanditError:
    """Exception correspondant à une catégorie d'erreur du moteur."""
    return ERROR_KINDS.get(kind, EngineInternalError)(message)
