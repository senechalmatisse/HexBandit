# ADR 0010 - Statistiques : modèle mixte, splines, puissance, Elo, plan adaptatif

> **En bref.** Quatre outils complètent le modèle à deux pentes,
> tous écrits avec numpy et scipy et validés sur données simulées. Ils
> montrent que les intervalles de la V1 étaient un peu trop étroits (sans
> changer ses conclusions), traduisent les résultats en points Elo, et
> permettent d'estimer r* avec 1,4 à 9 fois moins de parties.

| Outil | Question à laquelle il répond | Module |
|---|---|---|
| [Modèle mixte](#modèle-mixte) | les intervalles de r* sont-ils honnêtes malgré la surdispersion ? | `models.fit_mixed_model` |
| [Splines monotones](#splines-monotones) | r* dépend-il de la forme imposée à la courbe ? | `models.fit_monotone_splines` |
| [Analyse de puissance](#analyse-de-puissance) | combien de parties faut-il pour une précision donnée ? | `models.power_analysis` |
| [Échelle Elo](#échelle-elo) | combien valent le trait et un doublement de budget, en points Elo ? | `elo` |
| [Plan adaptatif](#plan-adaptatif) | peut-on mesurer r* avec moins de parties ? | `adaptive`, `hexbandit adaptive` |

## Contexte

Le modèle à deux pentes de la V1 ([rapport § 3](../rapport_scientifique.md#3-protocole-expérimental)) décrit bien les courbes P(premier gagne) en fonction de x = log₂ r, mais son test d'adéquation le rejette dans 8 groupes sur 18 : avec 1 800 à 3 600 parties par groupe, de petits écarts à la forme logistique deviennent significatifs (surdispersion entre cellules). Les intervalles de r* calculés comme si chaque cellule suivait exactement le modèle sont alors trop étroits. Par ailleurs, la grille fixe r = 2^k joue la plupart des parties loin de l'équilibre, et les résultats ne sont pas exprimés sur une échelle de force commune.

## Décisions

### Modèle mixte

Pour chaque groupe g (algorithme, B0, N), le modèle à deux pentes, plus un effet aléatoire par cellule c commun à tous les groupes :

```math
\operatorname{logit} P_c = a_g + b_{-,g}\,\min(x_c, 0) + b_{+,g}\,\max(x_c, 0) + u_c,
\qquad u_c \sim \mathcal{N}(0, \sigma^2)
```

* **Estimation.** La vraisemblance marginale de chaque cellule (binomiale intégrée sur u_c) est calculée par quadrature de Gauss-Hermite à 24 nœuds ; à σ fixé, les paramètres de chaque groupe sont estimés par BFGS avec gradient analytique (départ : régression logistique de Firth). σ est estimé par vraisemblance profilée (optimisation bornée en log σ sur [10⁻³ ; 3]), son intervalle par le seuil χ²₁(0,95)/2 de la vraisemblance profilée.
* **Test de σ = 0** (le modèle sans effet aléatoire) : rapport de vraisemblance, de loi ½χ²₀ + ½χ²₁ puisque σ = 0 est au bord de l'espace des paramètres.
* **r\* et son intervalle** par simulation (4 000 tirages) de la loi normale asymptotique des paramètres du groupe, de covariance l'inverse de la hessienne de la vraisemblance marginale : les intervalles s'élargissent d'autant que la surdispersion est forte.

Un modèle bayésien hiérarchique complet (a priori sur les pentes par taille et par algorithme) reste possible quand PyMC sera disponible ; le GLMM fréquentiste répond déjà à la question pratique : des intervalles honnêtes.

### Splines monotones

```math
\operatorname{logit} P = a + \sum_j w_j\, I_j(x), \qquad w_j \ge 0
```

Base d'I-splines (sommes cumulées de B-splines de degré 2, nœuds intérieurs aux quartiles des x du groupe), ajustée par L-BFGS-B sous contraintes : une courbe croissante sans forme imposée. r* est la racine de P = 0,5 ; l'intervalle vient d'un bootstrap paramétrique des cellules (200 tirages). C'est un contrôle : si r* ne dépend pas du modèle paramétrique, les deux estimations concordent (graphique G7).

### Analyse de puissance

Nombre de parties par cellule nécessaires, sur la grille fixe actuelle, pour une demi-largeur donnée de l'intervalle à 95 % de log₂ r* (0,1 par défaut) et de P à r = 1 (0,03), d'après l'information de Fisher du modèle à deux pentes estimé : de quoi dimensionner la campagne complète et juger l'intérêt du plan adaptatif.

### Échelle Elo

Modèle de Bradley-Terry avec avantage du trait, par (algorithme, N) :

```math
\operatorname{logit} P(\text{premier gagne}) = \theta(B_{\text{premier}}) - \theta(B_{\text{second}}) + \gamma
```

* Un niveau θ par budget effectivement joué (θ = 0 pour le plus petit) ; les deux plans B0 = 256 et B0 = 2 048 partagent les budgets communs (2 048 est joué comme r·256 et comme B0) et tiennent sur une seule échelle.
* Exprimé en Elo (θ · 400 / ln 10) : valeur du trait γ, gain d'un doublement de budget, déviance contre le modèle saturé.
* Le ratio d'équilibre s'en déduit : θ(r*·B0) = θ(B0) − γ.
* La même fonction classe les joueurs d'un tournoi toutes rondes (`hexbandit tournament`, amélioration 7.7), en prenant pour référence le joueur au taux de victoire le plus proche de 50 %.

### Plan adaptatif

Pour chaque groupe, des lots de parties sont joués au rapport le plus informatif sur r* :

1. modèle local logit P = β · (x − α), avec α = log₂ r* et β > 0 ;
2. loi a posteriori sur une grille (α uniforme sur l'intervalle étudié, log β normal) ;
3. rapport du lot suivant par la méthode Psi (Kontsevich et Tyler, 1999) : celui qui minimise l'entropie attendue de la loi de α ; le budget du premier joueur est arrondi à l'entier, et c'est le rapport réellement joué qui entre dans le modèle ;
4. arrêt quand l'intervalle de crédibilité à 95 % de α est plus étroit que `target_width`, ou à `max_games` parties.

Chaque lot est un match déterministe (graine dérivée de la graine maître, du groupe et du numéro du lot) : l'étude est reproductible et reprend après une interruption en vérifiant qu'elle refait les mêmes choix. `--compare` confronte les r* obtenus à ceux d'une campagne à grille fixe.

> [!NOTE]
> Le modèle local logistique symétrique est une approximation : il ignore
> l'asymétrie des pentes, faible près de l'équilibre. Le contrôle est la
> comparaison avec la grille fixe.

<p align="center">
  <img src="../figures/rapport/plan_adaptatif.png" width="720" alt="Plan adaptatif : rapports joués et intervalle de log2 r* au fil des lots">
</p>

**Résultat** sur les 20 groupes de la campagne complète (`results/adaptatif`, [rapport § 8.4](../rapport_scientifique.md#84-puissance-et-plan-adaptatif)) : 26 480 parties contre 52 200 pour la grille, de 1,4 à 9,4 fois moins de parties à précision égale, r* concordants dans 19 groupes sur 20.

> [!WARNING]
> Le groupe discordant s'est arrêté après 360 parties : avec la règle
> d'arrêt sur la largeur, quelques lots extrêmes au début peuvent suffire à
> conclure trop tôt. Relever `min_steps` est le remède simple.

## Conséquences

* `hexbandit report` ajoute ces analyses au rapport de campagne (`--v1-only` pour le rapport de la V1 seul) ; les analyses initiales sont inchangées.
* Graphiques G6 (échelle Elo) et G7 (r* selon le modèle), et graphique du plan adaptatif (rapports joués, intervalle de α au fil des lots).
* Ces analyses supposent des parties indépendantes et un effet de cellule normal ; la quadrature et l'optimisation sont validées sur des données simulées (σ et pentes retrouvés dans leurs intervalles, couverture du plan adaptatif), pas contre une bibliothèque de référence.

---
<div align="center">
[← ADR 0009](0009-variante-chameleon.md) · [Index des ADR](README.md)
</div>
