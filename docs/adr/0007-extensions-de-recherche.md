# ADR 0007 - Extensions de la recherche: solveur, réutilisation, GRAVE, parallélisme, coup final

> **En bref.** Cinq extensions classiques du MCTS, toutes désactivées par
> défaut et toutes déterministes. À budget égal en 9 × 9, **GRAVE** est la
> plus utile (+52 Elo) ; le solveur et la réutilisation de l'arbre aident un
> peu ; le parallélisme à la racine sert à aller plus vite, pas à mieux jouer.

| Option | Ce qu'elle fait | Effet mesuré (9 × 9, 1 000 simulations) |
|---|---|---|
| [MCTS-Solver](#mcts-solver) | prouve les positions gagnées, perdues ou nulles | +19 Elo |
| [Réutilisation](#réutilisation-du-sous-arbre) | garde le sous-arbre d'un coup à l'autre | +13 Elo |
| [GRAVE](#grave) | emprunte l'AMAF d'un ancêtre mieux informé | **+52 Elo** (R = 50) |
| [Parallélisme à la racine](#parallélisme-à-la-racine) | plusieurs arbres indépendants, fusionnés | −57 Elo à budget total égal |
| [Coup final](#choix-du-coup-final) | « secure » ou « max-robust » au lieu du plus visité | +8 et −46 Elo |

## Principe commun

Toutes ces options sont **désactivées par défaut** : une configuration normale donne les mêmes parties au bit près (test d'empreintes, hash golden, `hexbandit verify`). La configuration normalisée d'un joueur ne mentionne une option que si elle diffère du défaut, si bien que les identifiants de cellules et les graines des campagnes normales restent valables.

Elles s'appliquent à tout jeu (Hex, Reverse Hex, Chameleon, Quarto ; règle du gâteau comprise), sauf GRAVE, qui demande l'AMAF.

| Option (CLI `--blue rave:1000,…`) | Champ JSON du joueur | Défaut |
|---|---|---|
| `solver` | `"solver": true` | non |
| `reuse` | `"reuse_tree": true` | non |
| `grave=R` | `"grave_ref": R` | 0 (MC-RAVE) |
| `trees=K` | `"trees": K` (1 à 256) | 1 |
| `final=secure`, `secure_a=A` | `"final_move": "secure", "secure_a": A` | plus visité ; A = 1 |
| `final=max_robust` | `"final_move": "max_robust"` | plus visité |

## MCTS-Solver

Variante à bornes de score (Cazenave et Saffidine, 2010), qui gère aussi les parties nulles (Quarto) : chaque nœud porte une borne inférieure et une borne supérieure de sa valeur exacte pour le joueur qui y a le trait, en demi-points (0 défaite, 1 nul, 2 victoire), dans l'octet libre du nœud ([ADR 0003](0003-arbre-et-memoire.md)).

1. **Terminaux.** Un nœud terminal a pour bornes sa valeur exacte.
2. **Remontée.** Quand un nœud terminal est créé, les bornes remontent le chemin : celles d'un nœud sont le maximum, sur ses coups, des bornes de l'enfant vues de son côté (inversées si l'adversaire a le trait dans l'enfant), un coup non développé comptant pour [0 ; 2]. La remontée s'arrête au premier nœud inchangé.
3. **Nœuds résolus.** Un nœud résolu (bornes égales) se comporte comme un terminal : la descente s'y arrête et rétropropage sa valeur exacte, sans simulation.
4. **Élagage.** Dans un nœud dont au moins un coup a des bornes connues, la sélection écarte les coups dont la borne supérieure ne dépasse pas la borne inférieure du nœud : ils ne peuvent pas faire mieux qu'un coup déjà garanti.
5. **Arrêt anticipé.** La recherche s'arrête dès que la racine est résolue (moins de simulations que le budget ; le rapport l'indique par `proven`).
6. **Coup final.** Un coup prouvé gagnant s'il en existe (départagé par la stratégie de choix final) ; sinon la stratégie habituelle parmi les coups qui ne sont pas des défaites prouvées (tous si la position est perdue).

**Tests** (`crates/mcts/tests/v2_features.rs`) : sur des positions 4 × 4 tirées au hasard, toute borne calculée est comparée à la valeur minimax exacte ; un gain forcé en 3 × 3 est prouvé ; le morpion vide est prouvé nul.

## Réutilisation du sous-arbre

`Agent::observe` transmet à chaque agent les coups joués, les siens et ceux de l'adversaire (`runner::play_game` l'appelle pour les deux joueurs). Au coup suivant, `Mcts::search_after` descend de l'ancienne racine le long de ces coups :

* si chacun de ces coups a un nœud, et que le nœud atteint n'est pas terminal et a le même joueur au trait que la nouvelle position, son sous-arbre est gardé (`Tree::retain_subtree`, [ADR 0003](0003-arbre-et-memoire.md#réutilisation-de-larbre)) et l'algorithme reconstruit ses tables (`on_tree_rebuilt`, qui reçoit l'ancien numéro de chaque nœud) ;
* sinon l'arbre repart de zéro.

> [!IMPORTANT]
> Le budget compte les **nouvelles** simulations : un joueur qui réutilise
> son arbre dispose de N(racine) = budget + visites réutilisées
> (`reused_visits` dans le rapport). C'est pourquoi l'option reste désactivée
> dans les campagnes sur le ratio de budget : le budget nominal n'y serait
> plus le budget effectif, et l'écart dépendrait de la taille du plateau et
> de la prévisibilité de l'adversaire.

Le comportement reste déterministe.

## GRAVE

Généralisation de RAVE (Cazenave, 2015) : un nœud visité moins de R fois (`grave_ref`) remplace ses statistiques AMAF Ñ(s,a), Q̃(s,a) par celles de son plus proche ancêtre visité au moins R fois sur le chemin de la simulation (la racine à défaut), pour les coups du joueur qui a le trait **dans le nœud**.

* Le planning β est inchangé et reçoit le Ñ de la référence.
* Un coup sans statistiques dans la référence est traité comme un coup sans AMAF.
* R = 0 donne exactement MC-RAVE.

**Une table de plus par nœud.** Les arêtes d'un nœud ne portent que l'AMAF des coups de son joueur au trait (algorithme 2 de l'article) ; or la référence d'un nœud de Rouge peut être un nœud où Bleu a le trait. Avec GRAVE, chaque nœud tient donc en plus une table, indexée comme l'AMAF, des statistiques des coups **adverses** joués après lui (mise à jour dans la même passe que l'AMAF, avec la récompense de l'adversaire).

| | 9 × 9 | 14 × 14 |
|---|---|---|
| Table AMAF adverse, par nœud | 648 octets | 1,5 Ko |

Soit environ un tiers de mémoire en plus. La table n'existe pas quand R = 0. Avec la réutilisation de l'arbre, elle suit la renumérotation des nœuds.

**Tests** (`crates/mcts/src/algorithm.rs`) : la référence choisie à chaque niveau du chemin ; la table d'un nœud coïncide avec la somme de l'AMAF de ses enfants (à la simulation de création du nœud près), avant et après réutilisation de l'arbre ; le chemin général de sélection (GRAVE, solveur) choisit la même arête que le chemin rapide quand il ne change rien.

> [!NOTE]
> **Historique.** Une première version prenait l'AMAF des arêtes de la
> référence quel que soit le joueur qui y avait le trait : un nœud de Rouge
> lisait alors les statistiques des coups de Bleu, du point de vue de Bleu.
> Une deuxième, restreinte aux ancêtres du même joueur, était correcte mais
> plus faible ([mesures](#mesures)). La version livrée suit Cazenave.

## Parallélisme à la racine

`trees = K` lance K arbres indépendants dans des threads (`std::thread::scope`) :

* chaque arbre a son générateur, dérivé de celui du joueur (`derive_seed`, flux 0x5052 + k) ;
* chaque arbre reçoit la k-ième part du budget (`Budget::share` : simulations réparties, les premières parts en reçoivent une de plus si la division ne tombe pas juste ; même durée pour un budget en temps).

Les statistiques de la racine sont ensuite fusionnées **dans l'ordre des arbres** : N(s,a) additionnés, Q(s,a) moyennés avec N(s,a) pour poids et Q̃(s,a) avec Ñ(s,a), bornes du solveur combinées (la meilleure borne inférieure et la meilleure borne supérieure). Le coup final est choisi sur ces statistiques fusionnées.

Le résultat ne dépend ni du nombre de cœurs ni de l'ordonnancement des threads : la partie est déterministe pour un K donné (mais change avec K).

**Non retenu.** L'arbre partagé avec perte virtuelle (non déterministe) aurait rompu la reproductibilité des parties, qui est une exigence du projet. Les parties d'une campagne restent parallélisées entre elles (`--threads`) : à nombre de cœurs fixé, `trees > 1` accélère un coup mais pas une campagne.

## Choix du coup final

En plus de « plus visité » (défaut) et « meilleure valeur » :

| Stratégie | Règle |
|---|---|
| `secure` | maximise la borne inférieure Q(s,a) − A/√N(s,a) parmi les coups visités (A = 1 par défaut ; A = 0 revient à « meilleure valeur ») ; égalités départagées par les visites |
| `max_robust` | le coup à la fois le plus visité et le mieux évalué s'il existe ; sinon celui dont le moins bon des deux rangs est le meilleur, à égalité le plus visité |

La recherche n'est pas prolongée jusqu'à l'accord des deux critères de `max_robust`, pour garder un budget fixe.

## Mesures

`scripts/hexlXcp03.sh` (ou `make validation`) : chaque option contre MC-RAVE de référence (b = 0,02), 1 000 simulations par coup des deux côtés, 1 000 parties, premier coup alterné ; bilans dans `experiments/validation/options_v2.json`, analyse dans le [rapport scientifique](../rapport_scientifique.md#86-options-de-recherche--force-à-budget-égal).

| Option | 9 × 9 : victoires [IC 95 %] | 14 × 14 |
|---|---|---|
| `solver` | 52,7 % [49,6 ; 55,8] | |
| `reuse` | 51,8 % [48,7 ; 54,9] | 52,3 % [49,2 ; 55,4] |
| `grave=5` | 52,5 % | |
| `grave=10` | 54,1 % | |
| `grave=25` | 55,1 % | |
| `grave=50` | **57,5 %** [54,4 ; 60,5] | |
| `grave=100` | 53,3 % | |
| `grave=200` | 55,3 % | |
| `trees=2` (budget total égal) | 41,8 % [38,8 ; 44,9] | |
| `final=secure` | 51,2 % [48,1 ; 54,3] | |
| `final=max_robust` | 43,4 % [40,4 ; 46,5] | |

* **GRAVE** gagne pour tous les seuils, le plus à R = 50 (+52 Elo). La version intermédiaire restreinte aux ancêtres du même joueur perdait (41,9 % à R = 50, 40,8 % à R = 200) : la référence de l'adversaire, plus proche, est celle qui apporte le gain.
* **La réutilisation** ajoute en moyenne 8 % (9 × 9) et 5 % (14 × 14) de visites au budget (`experiments/validation/reutilisation_arbre.txt`), d'où un gain faible, cohérent avec l'échelle Elo des budgets.
* **Le parallélisme à la racine** perd à budget total égal ; il ne sert qu'à aller plus vite avec plusieurs cœurs.

---
<div align="center">
[← ADR 0006](0006-determinisme-multiplateforme.md) · [Index des ADR](README.md) · [ADR 0008 →](0008-connaissance-a-priori-hex.md)
</div>
