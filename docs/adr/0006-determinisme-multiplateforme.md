# ADR 0006 - Déterminisme multiplateforme : logarithme interne

> **En bref.** Le seul calcul du moteur qui pouvait différer d'un système à
> l'autre était le logarithme d'UCT, emprunté à la bibliothèque mathématique
> du système. Il est remplacé par un portage de `logf` de la glibc, vérifié
> identique au bit près sur les 2,1 milliards de `f32` positifs : aucune
> partie ne change sous Linux, et les parties ne dépendent plus du système.

## Contexte

Une partie est entièrement déterminée par sa configuration et sa graine ([ADR 0005](0005-semantique-mcts-rave.md)) : c'est ce qui permet de rejouer une tranche de campagne (`hexbandit verify`) et de comparer deux versions du moteur au bit près (`crates/mcts/tests/fingerprint.rs`, hash golden). Cette garantie n'était vérifiée que sous Linux.

**Ce qui est sûr.** Les opérations de base de l'IEEE 754 (addition, multiplication, division, racine carrée, conversions) sont correctement arrondies : leur résultat est le même sur tout processeur. Rust ne contracte pas `a * b + c` en FMA de lui-même et n'utilise pas la pile x87 sur les cibles 64 bits.

**Ce qui ne l'est pas.** `f32::ln` appelle `logf` de la bibliothèque mathématique du système (glibc, libm de macOS, CRT de Windows, musl), dont les arrondis ne sont pas normalisés. Le moteur n'utilise qu'une fonction transcendante : ln N(s), dans le terme d'exploration d'UCT et d'UCT-RAVE. MC-RAVE glouton (c = 0) et les plannings de β n'utilisent que des opérations de base. **Les parties UCT pouvaient donc différer selon le système.**

## Options examinées

| Option | Pour | Contre |
|---|---|---|
| Table de ln N pour N ≤ N_max | exacte, rapide | taille bornée (budget de 2²⁰ simulations et plus), cas particulier au-delà |
| Crate `libm` (portage de musl) | prête | absente hors ligne ; musl ≠ glibc : toutes les parties UCT auraient changé |
| **Portage de `logf` de la glibc** | Aucune partie ne change là où les campagnes ont été jouées | à vérifier exhaustivement |

## Décision

`game_core::math::ln(x: f32) -> f32` porte `logf` de la glibc (bibliothèque *optimized routines* d'Arm, utilisée par la glibc depuis la version 2.28) :

1. réduction x = 2^k · z ;
2. choix d'un des 16 sous-intervalles de [0,7 ; 1,4] par les bits de poids fort de la mantisse ;
3. ln x = k · ln 2 + ln c + P(z/c − 1), avec un polynôme P de degré 3.

Le tout en double précision, avec les seules opérations de base ; les constantes sont reprises en bits (`__logf_data`). Cas particuliers : ln 1 = +0, ln 0 = −∞, ln(+∞) = +∞, x < 0 ou NaN → NaN, dénormaux normalisés comme dans la glibc.

UCT et UCT-RAVE appellent `math::ln` ; aucun autre code du moteur n'appelle de fonction de la bibliothèque mathématique (vérifié par recherche dans les sources). Les statistiques Python ne participent pas aux parties.

## Vérification

| Vérification | Résultat |
|---|---|
| Exhaustive : `cargo test -p game-core --release -- --ignored` | identique au bit près à `logf` de la glibc 2.39 pour les 2 139 095 039 valeurs `f32` positives finies, dénormaux compris ; ce test tourne dans la CI Linux |
| Variante FMA de la glibc | une version du portage écrite avec `mul_add` a été comparée une fois, sur toutes les valeurs, à la version sans FMA : aucun écart |
| Résultats inchangés sous Linux | test d'empreintes (arbres complets, 9 configurations dont UCT et UCT-RAVE), hash golden, `hexbandit verify` sur la campagne réduite (dont les tranches UCT) |
| macOS et Windows | la CI exécute le test d'empreintes et rejoue six tranches jouées sous Linux, versionnées dans `experiments/verification` (trois UCT, trois MC-RAVE ; 5 × 5, 7 × 7 et 9 × 9) |

La glibc choisit à l'exécution une variante compilée avec FMA sur les processeurs récents (c'est celle de la machine de développement et des machines de la CI) : la comparaison des deux variantes montre que le résultat ne dépend pas de celle qui était présente sur la machine qui a joué les campagnes.

> [!WARNING]
> La vérification sous macOS et Windows n'a pas pu être exécutée dans l'environnement de développement ;
> elle le sera au premier passage de la CI.

## Conséquences

* Le moteur ne dépend plus de la bibliothèque mathématique du système : une partie jouée sous un système doit se rejouer à l'identique sous un autre, au moins sur les cibles 64 bits (x86-64, AArch64).
* Coût : un appel par nœud sélectionné pour UCT (le logarithme ne dépend que de N(s)), sans différence mesurable de débit.
* Toute nouvelle fonction transcendante dans le moteur (planning de β, priors, réglages par taille) devra passer par `game_core::math` avec la même vérification, sinon le déterminisme multiplateforme est perdu.

---
<div align="center">
[← ADR 0005](0005-semantique-mcts-rave.md) · [Index des ADR](README.md) · [ADR 0007 →](0007-extensions-de-recherche.md)
</div>
