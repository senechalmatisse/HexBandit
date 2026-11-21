# ADR 0005 - Sémantique précise de UCT et MC-RAVE

> **En bref.** L'article de Gelly et Silver laisse certains cas non définis
> (action jamais essayée, AMAF vide, fin exacte d'une simulation par
> remplissage…). Voici les choix faits, tous testés dans `crates/mcts/tests/engine.rs`.

## Valeurs et points de vue

Récompenses binaires (1 victoire, 0,5 nul, 0 défaite) du point de vue du joueur qui joue le coup : chaque nœud maximise, sans alternance min/max.

## UCT

```math
Q^{\oplus}(s,a) = Q(s,a) + c\,\sqrt{\frac{\ln N(s)}{N(s,a)}}
```

Une action jamais essayée a un bonus infini.

## MC-RAVE

```math
Q^{*}(s,a) = \bigl(1 - \beta(s,a)\bigr)\,Q(s,a) + \beta(s,a)\,\tilde{Q}(s,a)
```

| Cas | Valeur retenue |
|---|---|
| N(s,a) = 0 (pas de valeur Monte-Carlo) | β = 1 et Q* = Q̃ (c'est aussi la limite de l'éq. 47) |
| Ñ(s,a) = 0 (pas d'AMAF) | Q* = Q |
| N(s,a) = Ñ(s,a) = 0 | Q* = +∞ (action à essayer) |

* **Éq. 19** utilise N(s), le nombre de visites du nœud parent.
* **UCT-RAVE** (c > 0) : bonus c·√(ln N(s) / max(N(s,a), 1)), prolongée par continuité pour N(s,a) = 0. Un bonus infini forcerait à essayer chaque coup avant d'utiliser l'AMAF.
* **Mise à jour AMAF** : pour chaque nœud s_t du chemin, chaque coup a_u avec u ≥ t joué par le joueur au trait en s_t (dans l'arbre et dans la simulation) met à jour Ñ(s_t, a_u) et Q̃(s_t, a_u), une seule fois par case ; un coup illégal en s_t n'est pas compté. Le premier coup de la simulation est attribué au nouveau nœud.

## Trace exacte avec la politique de remplissage

La politique `HexFillRollout` remplit le plateau au hasard et ne vérifie la connexion qu'une fois. Pour UCT, seule l'issue compte. Pour RAVE, la trace s'arrête au coup qui termine réellement la partie : le tirage est rejoué dans l'ordre avec un marquage des pierres reliées à leur premier bord, qui s'arrête au coup gagnant.

Le test de contrat `check_rollout_contract` vérifie que la trace rejouée est légale, que le joueur de chaque coup est correct et que la partie se termine exactement au dernier coup, avec le même vainqueur.

> [!NOTE]
> L'équivalence « remplissage aléatoire » / « partie aléatoire » repose sur
> deux propriétés du Hex : un seul camp est connecté sur un plateau plein, et
> une connexion ne se défait pas. Elle est vérifiée statistiquement
> (40 000 simulations de chaque politique, écart inférieur à 4 écarts-types).

## Choix final

Par défaut le coup le plus visité (égalités départagées par la valeur) ; `best_value` choisit la meilleure valeur parmi les coups visités. `secure` (borne inférieure Q − A/√N) et `max_robust` ([ADR 0007](0007-extensions-de-recherche.md#choix-du-coup-final)).

## Réutilisation de l'arbre

Option `reuse_tree` (désactivée par défaut et dans les campagnes) : le budget compte les nouvelles simulations et le rapport donne les visites réutilisées ([ADR 0007](0007-extensions-de-recherche.md#réutilisation-du-sous-arbre)) |

---
<div align="center">
[← ADR 0004](0004-reglages-uct-rave.md) · [Index des ADR](README.md) · [ADR 0006 →](0006-determinisme-multiplateforme.md)
</div>
