"""hexbandit : pilotage, analyse et visualisation du moteur Hex / MCTS / MC-RAVE.

Le calcul (règles, recherche, parties) est fait par la bibliothèque native
Rust ; Python orchestre les campagnes, analyse les résultats et les affiche.
Toute dépendance au moteur passe par le port :class:`hexbandit.domain.ports.EnginePort`.
"""

from __future__ import annotations

__version__ = "0.2.0"

__all__ = ["__version__"]
