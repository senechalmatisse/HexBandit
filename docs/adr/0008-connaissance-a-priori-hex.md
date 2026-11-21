# ADR 0008 - Connaissance a priori propre au Hex

> **En bref.** À la création d'un nœud, chaque coup reçoit une valeur
> initiale tirée de motifs locaux du Hex : éviter les cases mortes, sauver
> ses ponts, couper ceux de l'adversaire, en créer de nouveaux. Gain mesuré :
> **+55 Elo en 9 × 9**, +27 en 14 × 14, pour une création de nœuds plus chère.

## Décision

### Les motifs (`game_hex::patterns`, indépendant du moteur)

Les six voisins d'une case forment un anneau. Une case hors du plateau est remplacée par le bord correspondant, qui appartient au joueur qui doit le relier (au coin de deux bords de joueurs différents, la couleur est traitée comme inconnue, par prudence).

| Motif | Définition | Ce qu'il faut en faire |
|---|---|---|
| **Case morte** | pour toute couleur future de ses voisins vides, les voisins de chaque couleur forment un seul arc de l'anneau : ils sont déjà reliés entre eux sans elle, et la case ne relie rien de nouveau, quelle que soit sa couleur | l'éviter : la jouer revient à passer, ce qui n'est jamais avantageux au Hex |
| **Pont sauvé** | deux pierres du joueur (ou une pierre et son bord) ont deux voisins communs, dont l'un vient d'être pris par l'adversaire | jouer l'autre sauve le pont |
| **Pont coupé** | même situation pour un pont adverse déjà entamé par le joueur | prendre le second voisin commun coupe le pont |
| **Nouveau pont** | la pierre posée serait à distance de pont d'une pierre du joueur, les deux voisins communs étant vides | connexion virtuelle |
| **Gabarit de bord** | case de la deuxième rangée d'un bord du joueur dont les deux voisins de la première rangée sont vides | pont vers le bord |

> [!NOTE]
> Le critère de case morte est seulement **suffisant** (il ne trouve pas
> toutes les cases mortes, ni les cases capturées ou dominées de l'analyse
> des cases inférieures) mais **sûr** : un test le compare à la vérité
> terrain calculée par énumération de tous les remplissages des cases vides,
> sur des positions 4 × 4 tirées au hasard ; aucune case déclarée morte ne
> change le vainqueur d'un remplissage.

### Le prior (`bindings::registry::HexPrior`)

Chaque coup prend la valeur H et la confiance C (en simulations équivalentes, pour Q comme pour Q̃) du **premier** motif qui s'applique, dans cet ordre :

| Motif | H (valeur du coup pour qui le joue) | C par défaut |
|---|---|---|
| case morte | 0,0 | 40 |
| pont sauvé | 1,0 | 30 |
| pont coupé | 0,9 | 20 |
| nouveau pont ou gabarit de bord | 0,75 | 10 |
| autre | 0,5 | 0 (pas de prior) |

Les valeurs et confiances sont choisies à la main, pas réglées (l'optimisation automatique des réglages, amélioration 3.13, reste à faire).

### Configuration

| Où | Syntaxe |
|---|---|
| Campagne TOML | `prior = { kind = "hex" }` |
| JSON | `"prior": {"kind": "hex", "dead": 40, "save": 30, "cut": 20, "bridge": 10, "even": 0}` |
| Ligne de commande | `prior=hex`, puis `prior.dead=60`, `prior.save=…` |

`prior=even:M` reste l'heuristique de l'article. Le prior du Hex s'applique au Hex avec ou sans règle du gâteau (le coup « échange » n'en reçoit pas) ; il est refusé avec un message clair pour Reverse Hex (les motifs s'y inversent), Chameleon et Quarto.

## Coût

Les motifs sont évalués pour chaque coup à la création de chaque nœud : lecture de l'anneau des six voisins ; une case morte n'est cherchée que si au plus quatre voisins sont de couleur inconnue, soit au plus 16 colorations. C'est sans effet sur la simulation, mais la création des nœuds est plus chère.

| 1 000 parties sur 2 cœurs, 1 000 simulations par coup | Sans prior | Avec prior |
|---|---|---|
| 9 × 9 | 29 à 35 s | 77 s (× 2,5) |
| 14 × 14 | 137 s | 403 s (× 3) |

C'est le prochain point à optimiser si l'option devient la valeur par défaut : motifs précalculés par case et mis à jour au coup, au lieu d'être recalculés par nœud.

## Mesures

Contre MC-RAVE de référence (b = 0,02), 1 000 simulations par coup des deux côtés, 1 000 parties (`scripts/hexlXcp03.sh`) :

| Plateau | Victoires [IC 95 %] | Elo | p |
|---|---|---|---|
| 9 × 9 | **57,9 %** [54,8 ; 60,9] | +55 | 3·10⁻⁷ |
| 14 × 14 | **53,9 %** [50,8 ; 57,0] | +27 | 0,007 |

Dans le tournoi toutes rondes en 9 × 9 (`experiments/tournoi_9x9.toml`), le prior et GRAVE (R = 50) gagnent chacun une soixantaine d'Elo sur MC-RAVE, mais réunis (avec le solveur) ils ne s'additionnent pas.

---
<div align="center">
[← ADR 0007](0007-extensions-de-recherche.md) · [Index des ADR](README.md) · [ADR 0009 →](0009-variante-chameleon.md)
</div>
