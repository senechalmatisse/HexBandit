# ADR 0004 - Réglages de UCT et de MC-RAVE

> **En bref.** Les deux algorithmes sont réglés par des matchs avant toute
> campagne : **b = 0,02** pour le biais de MC-RAVE (éq. 47) et **c = 0,25**
> pour l'exploration d'UCT, qui bat la valeur par défaut √2 dans 87,5 % des
> parties. Comparer un UCT mal réglé à un MC-RAVE réglé biaiserait H4.

## Contexte

L'article indique que le biais RAVE b (éq. 47) se règle empiriquement et que k ≥ 1000 est le meilleur réglage du planning manuel (éq. 19). La constante d'exploration par défaut d'UCT à √2.

## Mesures

Hex 9 × 9 sauf mention, joueurs alternant le premier coup. Données brutes : `experiments/validation/reglage_rave.json` et `reglage_uct.json` (commande `hexbandit match`).

### MC-RAVE : biais b et planning

MC-RAVE contre UCT (c = √2), 1 000 simulations par coup, 100 parties : MC-RAVE gagne 94 à 100 % des parties quel que soit le réglage. Ce plafond rend ce duel inutile pour régler RAVE ; la référence est donc MC-RAVE avec le planning manuel (k = 1 000), 200 parties par variante :

| Variante | Victoires | IC 95 % |
|---|---|---|
| éq. 47, b = 0 | 54,0 % | [47,1 ; 60,8] |
| éq. 47, **b = 0,02** | **60,5 %** | [53,6 ; 67,0] |
| éq. 47, b = 0,05 | 56,5 % | [49,6 ; 63,2] |
| éq. 47, b = 0,1 | 51,5 % | [44,6 ; 58,3] |
| éq. 47, b = 0,2 | 26,5 % | [20,9 ; 33,0] |
| éq. 19, k = 300 | 23,5 % | [18,2 ; 29,8] |
| éq. 19, k = 3 000 | 61,0 % | [54,1 ; 67,5] |

Duels directs : b = 0,02 contre k = 3 000 : 53,5 % (non significatif) ; b = 0,02 contre b = 0,05 : 51,5 % (256 simulations) et 56 % (13 × 13, 2 048 simulations, non significatif).

### UCT : constante d'exploration c

UCT (variante de c) contre UCT (c = √2), 1 000 simulations, 200 parties :

| c | Victoires contre c = √2 |
|---|---|
| 1 | 58,5 % |
| 0,5 | 76,5 % |
| **0,25** | **87,5 %** |

c = 0,1 ne gagne que 41 % contre c = 0,25. c = 0,25 bat aussi c = 0,5 à 256 simulations (66,5 %) et en 11 × 11 à 2 048 simulations (71 %).

## Décision

| Réglage | Valeur par défaut du moteur | Valeur des campagnes |
|---|---|---|
| Planning de β de MC-RAVE | éq. 47 | éq. 47 |
| Biais b de MC-RAVE | 0,02 (était 0,05 provisoirement) | 0,02 |
| Exploration de MC-RAVE | c = 0 (MC-RAVE glouton, comme l'article) | c = 0 |
| Exploration d'UCT | √2 | **0,25** |

Les campagnes utilisent des joueurs réglés : comparer un UCT mal réglé à un MC-RAVE réglé biaiserait H4.

## Conséquences

Les deux réglages sont explicites dans les fichiers TOML des campagnes et recopiés, normalisés, dans chaque enregistrement de partie.

> [!NOTE]
> GRAVE (seuil R = 50) et la connaissance a priori du Hex ont aussi été mesurés contre ce MC-RAVE de référence
> ([ADR 0007](0007-extensions-de-recherche.md#mesures), [ADR 0008](0008-connaissance-a-priori-hex.md#mesures)) ; leurs valeurs restent choisies à la main.

---
<div align="center">
[← ADR 0003](0003-arbre-et-memoire.md) · [Index des ADR](README.md) · [ADR 0005 →](0005-semantique-mcts-rave.md)
</div>
