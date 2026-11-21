# ADR 0003 - Arbre en arène, mémoire et performance

> **En bref.** L'arbre de recherche est une arène indexée par `u32` : des
> nœuds de 16 octets et des arêtes rangées en colonnes
> (18 octets pour UCT, 22 pour MC-RAVE). Chaque optimisation a été mesurée
> et aucune n'a changé une seule partie : les empreintes des arbres complets
> sont identiques au bit près d'une version à l'autre.

**Évolution en un coup d'œil** (cachegrind, 10 000 simulations MC-RAVE depuis le plateau vide 14 × 14 ; mesures déterministes, comparables entre lignes) :

## Décision initiale

L'arbre est une arène de deux `Vec` indexés par `u32` :

* `Node` : N(s), indice et nombre d'arêtes, joueur au trait, résultat si terminal - 16 octets ;
* `Edge<M, X>` : coup, enfant, N(s,a) (`u32`), Q(s,a) (`f32`), 1/√N(s,a) précalculé pour UCB1, et les statistiques propres à l'algorithme `X` (rien pour UCT ; Ñ(s,a) et Q̃(s,a) pour RAVE).

Au Hex, un coup tient sur 2 octets : une arête occupe 20 octets (UCT) ou 28 octets (RAVE), tailles vérifiées par le test `memory_layout_matches_adr_0004`. RAVE ajoute par nœud une table « indice de coup → arête » de 2 octets par case. Un nœud 14 × 14 proche de la racine (≈ 196 arêtes) coûte donc environ 3,9 Ko (UCT) ou 5,9 Ko (RAVE), un peu plus que les 3 Ko estimés par le cahier des charges.

| Budget par coup | Nœuds (≈ 1 par simulation) | Mémoire UCT | Mémoire RAVE |
|---|---|---|---|
| 2 048 | 2 049 | 8 Mo | 12 Mo |
| 32 768 | 32 769 | 128 Mo | 193 Mo |
| 250 000 (limite par défaut) | 250 000 | ≈ 1 Go | ≈ 1,5 Go |

La limite `max_nodes` (défaut 250 000) borne la mémoire : au-delà, la recherche continue sans expansion et le rapport l'indique (`node_limit_reached`). Les capacités sont conservées entre deux coups (aucune allocation en régime établi).

### Optimisations retenues

Mesurées avec le banc `cargo bench -p mcts` :

| Étape | MC-RAVE 14 × 14 (simulations/s, un cœur) |
|---|---|
| Première version | 52 000 |
| Trace exacte par propagation au lieu d'un union-find | 55 000 |
| Table « coup → arête » par nœud (fin du parcours de toutes les arêtes et des branchements imprévisibles) | 70 000 - 81 000 |
| Écriture groupée de la trace, coups légaux tirés de la liste des cases vides | 83 000 |
| Passe chronologique unique (vainqueur + fin de partie), mélange au fil de l'eau | 84 000 - 96 000 |
| Q* calculé à la sélection (β n'est utile que pour les actions visitées) | **104 000** |

UCT atteint 185 000 à 222 000 simulations/s selon les mesures (machine virtuelle bruitée). Les deux dépassent l'objectif [M] de 100 000 ; l'objectif [S] de 300 000 n'est pas atteint depuis la position vide 14 × 14, mais l'est en cours de partie (moins de cases vides), où UCT mesure 360 000 à 560 000 simulations/s.

### Choix de comportement liés

* Les arêtes sont mélangées à la création du nœud (départage uniforme des égalités et des actions non essayées).
* Le premier coup de la simulation est attribué au nœud créé et son arête placée en tête : pour UCT, les actions essayées forment alors un préfixe de longueur N(s), d'où une sélection en O(1) tant que N(s) est inférieur au nombre d'arêtes.

## Mesures sur une autre machine

### Le constat

Sur une machine (Rust 1.100), l'ancien banc donnait UCT 138 000 et MC-RAVE 58 000 simulations/s pour MC-RAVE. La simulation par remplissage seule y tournait à 261 000/s contre 470 000 à 530 000 sur la machine de développement : ce code y est environ deux fois plus lent, et MC-RAVE souffre davantage.

### Le diagnostic

Cachegrind, avec un cache de dernier niveau simulé à 8 Mo comme sur cette machine et la prédiction de branchements simulée :

1. **Le banc mesurait la première recherche d'un moteur neuf** : l'arbre (30 000 nœuds, 160 Mo d'arêtes pour MC-RAVE) grandissait par réallocations successives, chacune copiant tout l'arbre. Une partie réelle réutilise le moteur d'un coup à l'autre, et criterion mesure après échauffement.
2. **Des branchements imprévisibles** dans les parcours de la simulation (vérification de connexion, propagation des pierres reliées) : sortie anticipée de `any()`, empilement conditionnel, nombre de voisins variable au bord du plateau.
3. **Le planning de β était choisi par un `match` à chaque arête.**

### Les changements

Tous **sans effet sur les résultats** : le test `crates/mcts/tests/fingerprint.rs` (empreinte au bit près de l'arbre complet pour 9 configurations, calculée avec l'ancien code), le hash golden et `hexbandit verify` (8 tranches sur 8 de la campagne réduite rejouées à l'identique, dont MC-RAVE en 11 × 11 et 14 × 14) sont inchangés.

* `Budget::expected_simulations` : le moteur réserve d'un coup la mémoire de l'arbre (et la table coup → arête de MC-RAVE) ; réservation faillible, simple indication.
* Parcours sans branchement dépendant des données : accumulation par « ou » logique, écriture inconditionnelle au sommet de la pile puis incrément conditionnel ; table de voisins complétée à 6 par une case sentinelle toujours vide (`Topology::padded`).
* `BetaSchedule::kind` : le planning est résolu une fois par nœud ; la boucle de sélection ne contient plus d'appel au planning.

### Les mesures

Cachegrind, 10 000 simulations depuis le plateau vide 14 × 14, moteur neuf :

| | Instructions | Mauvaises prédictions | Défauts D1 | Défauts dernier niveau (8 Mo) |
|---|---|---|---|---|
| UCT avant | 253 M | 2,11 M | 2,54 M | 2,45 M |
| UCT après | 259 M | 0,26 M | 0,64 M | 0,62 M |
| MC-RAVE avant | 806 M | 5,16 M | 5,61 M | 3,73 M |
| MC-RAVE après | 783 M | 4,37 M | 2,78 M | 0,92 M |

Débits sur la machine de développement (médiane de 5, alternance ancien et nouveau binaire pour neutraliser le bruit de la machine virtuelle) :

| | Avant | Après |
|---|---|---|
| Simulation par remplissage, sans trace | 470 000 | 860 000 |
| UCT, moteur chaud | 290 000 | 330 000 - 390 000 |
| MC-RAVE (éq. 47), moteur chaud | 122 000 | 132 000 - 149 000 |
| MC-RAVE (éq. 47), moteur neuf | ≈ 105 000 | ≈ 109 000 |

### Les essais écartés

Plus lents, mesurés :

* mise à jour AMAF par parcours des arêtes du nœud et table de dernière position du coup (au lieu de la table coup → arête) : deux fois plus d'arêtes examinées, avec une branche imprévisible (65 000/s) ou une division à chaque arête (69 000/s) ;
* sélection en deux passages (arêtes non visitées sans branche, puis visitées) : 16 % d'instructions et 5 % de mauvaises prédictions en plus.

Il restait environ 440 mauvaises prédictions par simulation MC-RAVE, dont la moitié dans la sélection (« arête visitée ou non », imprévisible car les arêtes sont mélangées). Le banc affiche désormais le débit moteur chaud et le débit moteur neuf.

## Révision : arêtes en colonnes

### Disposition

Les arêtes ne sont plus un `Vec<Edge>` mais des colonnes parallèles (structure de tableaux) indexées comme avant par `first_edge + i` :

| Colonne | Type | Octets (Hex) | Présente pour |
|---|---|---|---|
| `moves` | coup | 2 | tous |
| `children` | `u32` | 4 | tous |
| `visits` N(s,a) | `u32` | 4 | tous |
| `values` Q(s,a) | `f32` | 4 | tous |
| `inv_sqrt` 1/√N(s,a) | `f32` | 4 | UCT et UCT-RAVE (c > 0) seulement |
| `extras` Ñ(s,a), Q̃(s,a) | `u32`, `f32` | 8 | MC-RAVE, UCT-RAVE, GRAVE |

| Algorithme | Octets par arête | V2 |
|---|---|---|
| UCT | 20 | **18** |
| MC-RAVE (c = 0) | 28 | **22** |
| UCT-RAVE | 28 | **26** |

Le rembourrage de la structure disparaît, et MC-RAVE glouton se passe de la colonne 1/√N, dont il n'a jamais besoin (c = 0 ; `TreeAlgorithm::needs_inv_sqrt`) : l'objectif de 24 octets de l'amélioration 4.3 est dépassé. Le test `memory_layout_matches_adr_0004` vérifie ces tailles par `Tree::bytes_per_edge`. Le nœud reste à 16 octets : l'octet libre reçoit les bornes du solveur ([ADR 0007](0007-extensions-de-recherche.md)).

| Budget par coup | Nœuds | Mémoire UCT | Mémoire MC-RAVE |
|---|---|---|---|
| 2 048 | 2 049 | 7 Mo | 10 Mo |
| 32 768 | 32 769 | 116 Mo | 154 Mo |
| 250 000 (limite par défaut) | 250 000 | ≈ 0,9 Go | ≈ 1,2 Go |

*14 × 14, ≈ 196 arêtes par nœud près de la racine, table coup → arête de MC-RAVE comprise. Ce sont des majorants : les nœuds profonds ont moins d'arêtes.*

### Sélection sans branchement

La sélection de MC-RAVE parcourt les arêtes par blocs de 32 : le score de chaque arête est écrit dans un tableau local avec des sélections par comparaison flottante (arête visitée ou non, AMAF disponible ou non), que le compilateur traduit en instructions de sélection conditionnelle, puis le maximum du bloc est cherché.

> [!IMPORTANT]
> L'ordre des opérations flottantes (Q* = Q + β · (Q̃ − Q)) et le départage
> des égalités (premier maximum dans l'ordre des arêtes) sont ceux de la V1 :
> **résultats inchangés au bit près** (test d'empreintes, hash golden, `hexbandit verify`).

GRAVE et le solveur, qui lisent les statistiques d'un ancêtre ou excluent des coups, passent par une boucle générale ; un test vérifie qu'elle choisit la même arête que le chemin rapide quand elle ne change rien.

Les essais de vectorisation explicite (masques de bits, 4 ou 32 voies fixes, itérateurs zippés) n'ont pas produit de code SIMD plus rapide : ils réintroduisaient des branchements ou empêchaient la vectorisation. Une variante de la disposition en colonnes avec la boucle à branchements donnait le même temps : le gain vient d'abord de la compacité (moins de lignes de cache par nœud).

### Mesures

Cachegrind, 10 000 simulations MC-RAVE (éq. 47) depuis le plateau vide 14 × 14, moteur neuf :

| | Instructions | Mauvaises prédictions | Défauts D1 |
|---|---|---|---|
| arêtes en structures | 783 M | 4,37 M | 2,77 M |
| colonnes, sélection sans branchement | 835 M | 2,84 M | 1,55 M |

Les instructions augmentent de 7 % (scores calculés pour toutes les arêtes du bloc), mais les mauvaises prédictions passent de 437 à 284 par simulation et les défauts de cache D1 baissent de 44 %.

Débits sur la machine de développement (simulations/s, 14 × 14, médianes de 5, binaires alternés ; machine virtuelle bruitée, d'où les intervalles) :

| | V1 | V2 |
|---|---|---|
| UCT, moteur chaud | 354 000 - 362 000 | 362 000 - 366 000 |
| MC-RAVE éq. 47, moteur chaud | 124 000 - 125 000 | 116 000 - 129 000 |
| MC-RAVE éq. 19, moteur chaud | 103 000 - 118 000 | 123 000 - 129 000 |
| UCT, moteur neuf | ≈ 250 000 | ≈ 332 000 |
| MC-RAVE, moteur neuf | 95 000 - 100 000 | 124 000 - 127 000 |

Le moteur neuf gagne le plus : l'arbre plus compact se réserve et se remplit plus vite. Machine au repos, le banc de la V2 mesure 480 000 simulations/s pour UCT et 134 000 pour MC-RAVE (`experiments/validation/banc_v2.txt`).

> [!NOTE]
> Les chiffres restent à remesurer sur la machine cible.
> Sur la machien où la V1 de MC-RAVE plafonnait à 58 000/s, la baisse des
> mauvaises prédictions et des défauts de cache devrait compter davantage
> que sur la machine de développement.

### Réutilisation de l'arbre

`Tree::retain_subtree` garde le sous-arbre du coup joué ([ADR 0007](0007-extensions-de-recherche.md)) : parcours en largeur depuis la nouvelle racine, renumérotation des nœuds et des arêtes, recopie des colonnes dans des tableaux neufs de même capacité (une allocation par colonne à chaque coup, sans réallocation pendant la recherche qui suit). La fonction rend l'ancien numéro de chaque nœud, pour que l'algorithme renumérote ses propres tables (GRAVE).

---
<div align="center">
[← ADR 0002](0002-pont-abi-c.md) · [Index des ADR](README.md) · [ADR 0004 →](0004-reglages-uct-rave.md)
</div>