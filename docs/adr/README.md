# Décisions d'architecture (ADR)

Chaque ADR décrit une décision : son contexte, les options examinées, ce qui a été retenu et ce que cela implique.

| ADR | Décision | Statut | En une phrase |
|---|---|---|---|
| [0001](0001-architecture.md) | Architecture Rust + Python | accepté | Rust calcule, Python analyse, un seul point de contact ; le moteur ne dépend que d'abstractions. |
| [0002](0002-pont-abi-c.md) | Pont par ABI C et ctypes | accepté | Bibliothèque partagée et JSON au lieu de PyO3, avec un chemin de migration en trois étapes. |
| [0003](0003-arbre-et-memoire.md) | Arbre en arène, mémoire, performance | accepté | Nœuds de 16 octets, arêtes en colonnes de 18 à 26 octets ; chaque optimisation mesurée, aucune partie changée. |
| [0004](0004-reglages-uct-rave.md) | Réglages d'UCT et de MC-RAVE | accepté | b = 0,02 pour MC-RAVE, c = 0,25 pour UCT, mesurés par matchs. |
| [0005](0005-semantique-mcts-rave.md) | Sémantique d'UCT et de MC-RAVE | accepté | Les cas que l'article laisse ouverts, tranchés et testés. |
| [0006](0006-determinisme-multiplateforme.md) | Déterminisme multiplateforme | accepté | Un logarithme portable, identique à la glibc au bit près, rend les parties indépendantes du système. |
| [0007](0007-extensions-de-recherche.md) | Extensions de la recherche | accepté | Solveur, réutilisation, GRAVE, parallélisme à la racine, coup final : désactivés par défaut, mesurés. |
| [0008](0008-connaissance-a-priori-hex.md) | Connaissance a priori du Hex | accepté | Cases mortes, ponts et gabarits de bord comme valeurs initiales : +55 Elo en 9 × 9. |
| [0009](0009-variante-chameleon.md) | Variante Chameleon | accepté | Un Hex aux pierres des deux couleurs, ajouté sans toucher le moteur. |
| [0010](0010-statistiques.md) | Statistiques | accepté | Modèle mixte, splines, puissance, Elo et plan adaptatif. |

**Par thème**

* Architecture et environnement : [0001](0001-architecture.md), [0002](0002-pont-abi-c.md)
* Moteur : [0003](0003-arbre-et-memoire.md), [0005](0005-semantique-mcts-rave.md), [0006](0006-determinisme-multiplateforme.md), [0007](0007-extensions-de-recherche.md)
* Force de jeu : [0004](0004-reglages-uct-rave.md), [0007](0007-extensions-de-recherche.md), [0008](0008-connaissance-a-priori-hex.md)
* Jeux : [0009](0009-variante-chameleon.md) et le [guide « ajouter un jeu »](../guide_ajouter_un_jeu.md)
* Statistiques : [0010](0010-statistiques.md) et le [rapport scientifique](../rapport_scientifique.md)
