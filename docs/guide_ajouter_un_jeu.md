# Guide : ajouter un jeu ou une variante

> **En bref.** Le moteur `mcts` est générique : il ne connaît que les traits
> de `game-core`. Ajouter un jeu ne modifie **aucun fichier** de `crates/mcts` ni de `crates/game-core` ;
> seuls le crate du jeu, le registre de `bindings` et, pour l'affichage,
> quelques lignes côté Python changent.
> Ce guide suit cet exemple pas à pas ([ADR 0009](adr/0009-variante-chameleon.md)).

**Deux chemins selon le jeu :**

| Votre jeu… | Chemin | Effort |
|---|---|---|
| ne change qu'une règle d'un jeu existant (résultat inversé, règle du gâteau) | [1. un décorateur](#1-variante-par-décorateur-exemple--reverse-hex) | quelques lignes |
| a ses propres règles | [2. un nouveau crate](#2-nouveau-jeu--lexemple-de-chameleon) | un crate et trois lignes de registre |

**Vue d'ensemble d'un nouveau jeu :**

| Étape | Fichier | Obligatoire ? |
|---|---|---|
| [2.1](#21-le-crate) Créer le crate | `crates/mon-jeu/Cargo.toml`, `Cargo.toml` racine | oui |
| [2.2](#22-règles--implémenter-game) Écrire les règles | `impl Game` | oui |
| [2.3](#23-rave--implémenter-amafgame-seulement-si-cest-pertinent) Ouvrir RAVE et GRAVE | `impl AmafGame` | si un coup a le même sens partout |
| [2.4](#24-règle-du-gâteau-facultatif--implémenter-piegame) Ouvrir la règle du gâteau | `impl PieGame` | non |
| [2.5](#25-politique-de-simulation) Accélérer les simulations | `impl RolloutPolicy` | non (`UniformRollout` suffit) |
| [2.6](#26-tests-de-contrat) Tester | `tests/contract.rs` | oui |
| [2.7](#27-exposition-au-registre-et-à-python) Brancher au registre et à Python | `bindings`, `hexbandit/domain/types.py` | oui |
| [3](#3-vérifier) Vérifier | `make verifier`, un match MC-RAVE contre UCT | oui |

## 1. Variante par décorateur (exemple : Reverse Hex)

Si la variante ne change qu'une règle d'un jeu existant, un décorateur de `game-core::decorators` suffit :

* `Misere<G>` inverse le résultat (Reverse Hex : relier ses bords fait perdre) ;
* `SwapRule<G>` ajoute la règle du gâteau (le jeu enveloppé implémente `PieGame::apply_swap`).

Reverse Hex tient en deux lignes dans `crates/game-hex/src/lib.rs` :

```rust
pub type ReverseHex = Misere<HexState>;
pub type ReverseHexRollout = MisereRollout<HexRollout>;
```

puis une ligne dans le registre (`crates/bindings/src/registry.rs`, fonction `dispatch`) et une valeur dans `GameKind` (`config.rs`).

## 2. Nouveau jeu : l'exemple de Chameleon

Chameleon se joue sur le plateau du Hex, mais chaque joueur pose à son tour une pierre **de l'une ou l'autre couleur** ; Bleu gagne avec une chaîne d'une même couleur d'Ouest en Est, Rouge avec une chaîne d'une même couleur du Nord au Sud, et si un coup crée les deux, son auteur gagne.

### 2.1 Le crate

```toml
# crates/game-chameleon/Cargo.toml
[package]
name = "game-chameleon"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[lints]
workspace = true

[dependencies]
game-core.workspace = true
game-hex.workspace = true   # cases, notation et voisinage du plateau
```

puis `"crates/game-chameleon"` dans `members` et `game-chameleon = { path = "crates/game-chameleon" }` dans `[workspace.dependencies]` du `Cargo.toml` racine.

### 2.2 Règles : implémenter `Game`

Le coup est un type `Copy + Eq + Debug` ; chez Chameleon, il est composé :

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChameleonMove {
    pub cell: Cell,      // case (game_hex::Cell)
    pub color: Player,   // couleur de la pierre, indépendante du joueur
}

impl Game for ChameleonState {
    type Move = ChameleonMove;
    fn current_player(&self) -> Player { self.to_move }
    fn legal_moves(&self, out: &mut Vec<ChameleonMove>) {
        out.clear();
        if self.outcome.is_some() { return; }
        for &c in &self.empties {
            for color in [BLUE, RED] {
                out.push(ChameleonMove { cell: Cell(c), color });
            }
        }
    }
    fn play(&mut self, mv: ChameleonMove) -> Result<(), GameError> {
        // erreurs typées, jamais de panique : GameOver, OutOfBounds, Occupied
        let mover = self.to_move;
        self.outcome = match self.place(cell, mv.color) {   // (Ouest-Est, Nord-Sud)
            (true, true) => Some(Outcome::Win(mover)),      // les deux : l'auteur gagne
            (true, false) => Some(Outcome::Win(BLUE)),
            (false, true) => Some(Outcome::Win(RED)),
            (false, false) => None,
        };
        self.to_move = mover.opponent();
        self.ply += 1;
        Ok(())
    }
    fn outcome(&self) -> Option<Outcome> { self.outcome }
    fn ply(&self) -> u32 { self.ply }
    fn random_legal_move<R: Rng + ?Sized>(&self, rng: &mut R, _buf: &mut Vec<ChameleonMove>)
        -> Option<ChameleonMove> { /* un seul tirage parmi 2 × (cases vides) */ }
}
```

> [!TIP]
> **Points d'attention**
>
> * Le moteur clone la racine à chaque simulation : garder un état compact
>   (Chameleon : un octet par case, la liste des cases vides et deux
>   union-find), et implémenter `Clone::clone_from` en réutilisant les
>   tampons si l'état contient des `Vec`.
> * Spécialiser `random_legal_move` si un tirage en O(1) est possible : c'est
>   la boucle la plus chaude des simulations.
> * Détecter la fin de partie de façon incrémentale. Chameleon garde un
>   union-find par couleur dont chaque racine retient le masque des bords
>   touchés ; des nœuds virtuels de bord, comme au Hex, seraient faux ici
>   (deux chaînes touchant le Nord seraient réunies par lui).
> * Un match nul est représenté par `Outcome::Draw` (récompense 0,5) ;
>   Chameleon n'en a pas.

### 2.3 RAVE : implémenter `AmafGame` seulement si c'est pertinent

L'AMAF suppose qu'un coup garde le même sens dans toutes les positions. C'est vrai d'une pierre de couleur donnée sur une case donnée :

```rust
impl AmafGame for ChameleonState {
    /// case × 2 + couleur : bleu en c4 et rouge en c4 sont deux coups distincts.
    fn move_index(&self, mv: ChameleonMove) -> usize {
        usize::from(mv.cell.0) * 2 + mv.color.index()
    }
    fn move_index_bound(&self) -> usize { 2 * self.cells.len() }
}
```

> [!NOTE]
> Un jeu dont les coups n'ont pas de sens hors contexte (Quarto : « poser une
> pièce et donner la suivante ») n'implémente pas `AmafGame` : le compilateur
> interdit alors `Rave<B>` pour ce jeu et seul UCT est disponible.

GRAVE et le solveur ([ADR 0007](adr/0007-extensions-de-recherche.md)) viennent sans rien ajouter : le premier ne demande que l'AMAF, le second que `outcome`.

### 2.4 Règle du gâteau (facultatif) : implémenter `PieGame`

```rust
impl PieGame for ChameleonState {
    /// La première pierre (r, c) passe en (c, r) en gardant sa couleur ; le
    /// premier joueur rejoue.
    fn apply_swap(&mut self) -> Result<(), GameError> { ... }
}
```

`SwapRule<ChameleonState>` et `SwapRollout` fournissent ensuite le coup « échange », sa notation et sa simulation.

### 2.5 Politique de simulation

`UniformRollout<ChameleonState>` fonctionne d'emblée (c'est celle de Chameleon : l'astuce du remplissage du Hex ne s'applique pas, la partie s'arrête à la première chaîne et son vainqueur dépend de l'ordre des coups). Une politique spécialisée implémente `RolloutPolicy<MonJeu>` et doit respecter le contrat : en mode `TraceMode::Full`, rejouer la trace depuis la position de départ termine la partie exactement au dernier coup, avec le même résultat.

### 2.6 Tests de contrat

```rust
use game_core::contract::{check_amaf_contract, check_game_contract, check_rollout_contract};

#[test]
fn chameleon_satisfies_the_contracts() {
    for size in [2, 3, 5, 7] {
        let initial = ChameleonState::new(size).unwrap();
        check_game_contract(&initial, 1, 50, 300).unwrap();
        check_amaf_contract(&initial, 2, 20).unwrap();
        check_rollout_contract(&initial, &mut UniformRollout::new(), 3, 100).unwrap();
    }
}
```

> [!IMPORTANT]
> Ajoutez un test qui recoupe la détection incrémentale de fin de partie par
> un calcul complet : `crates/game-chameleon/tests/contract.rs` rejoue
> 3 000 parties aléatoires et compare, après chaque coup, le résultat
> incrémental à un parcours en largeur du plateau. C'est ce test qui a révélé
> l'erreur des nœuds virtuels de bord.

### 2.7 Exposition au registre et à Python

Dans `crates/bindings` :

1. `GameKind::Chameleon` et sa lecture (`"kind": "chameleon"`) dans `config.rs` ;
2. `impl EngineGame for ChameleonState` dans `registry.rs` : position initiale, lecture d'une position, politique de simulation, notation des coups (`c4b`, `c4r`), position textuelle ; MC-RAVE est permis parce que le jeu implémente `AmafGame` ;
3. deux lignes dans `dispatch`, avec et sans règle du gâteau :

```rust
(GameKind::Chameleon, false) => visitor.visit::<ChameleonState>(spec),
(GameKind::Chameleon, true) => visitor.visit::<SwapRule<ChameleonState>>(spec),
```

Côté Python, ajouter `"chameleon"` au `Literal` de `GameSpec.kind` (`hexbandit/domain/types.py`) et aux choix `--kind` de la ligne de commande. La visualisation (`hexbandit.viz`, visionneuse web) lit seulement l'enregistrement JSON : Chameleon réutilise le plateau du Hex (`HexBoard(..., chameleon=True)`, qui lit la couleur dans le coup et non dans le joueur). Un jeu sur un autre plateau demande son propre `Renderer`.

## 3. Vérifier

```sh
make verifier                                                                # tous les contrôles de la CI
git diff --stat <commit-de-depart> -- crates/mcts crates/game-core           # doit être vide
make match JEU=chameleon TAILLE=7 A=rave:1000 B=uct:1000,c=0.25 PARTIES=200
make demo JEU=chameleon TAILLE=7                                             # une partie en image, en GIF et dans la visionneuse
```

Sans `make` : `cargo test --workspace`, puis `python -m pytest -q` et `python -m hexbandit match --kind chameleon …` depuis `python/`.

Le match sert de contrôle de bon sens : MC-RAVE doit battre UCT à budget égal si l'indice AMAF est bien choisi (Chameleon : 95,7 % sur 1 000 parties, `experiments/validation/chameleon.json`, produit par `scripts/hexlXcp03.sh`).
