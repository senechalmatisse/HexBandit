//! Règles du jeu de Hex.
//!
//! * [`HexState`] : plateau `n × n` en losange, alternance stricte, rejet typé
//!   des coups invalides, détection incrémentale de la victoire par union-find ;
//! * [`notation`] : coups `c4` et positions textuelles ;
//! * [`render_ascii`] : rendu texte du plateau ;
//! * [`HexFillRollout`] : politique de simulation optimisée pour le Hex ;
//! * [`patterns`] : motifs locaux (cases mortes, ponts, gabarits de bord),
//!   connaissance a priori pour MCTS ;
//! * variantes : règle du gâteau ([`HexSwap`]) et Reverse Hex ([`ReverseHex`]),
//!   obtenues par les décorateurs génériques de `game-core`.
//!
//! ```
//! use game_core::{Game, Outcome};
//! use game_hex::{BLUE, HexState, notation::parse_cell};
//!
//! let mut s = HexState::new(2).unwrap();
//! s.play(parse_cell("a1", 2).unwrap()).unwrap(); // Bleu
//! s.play(parse_cell("a2", 2).unwrap()).unwrap(); // Rouge
//! s.play(parse_cell("b1", 2).unwrap()).unwrap(); // Bleu relie Ouest-Est
//! assert_eq!(s.outcome(), Some(Outcome::Win(BLUE)));
//! ```
#![forbid(unsafe_code)]

pub mod notation;
pub mod patterns;
mod render;
mod rollout;
mod state;
mod topology;

use game_core::{
    Player,
    decorators::{Misere, MisereRollout, SwapRollout, SwapRule},
};

pub use render::render_ascii;
pub use rollout::{HexFillRollout, HexRollout};
pub use state::HexState;
pub use topology::Geometry;

/// Bleu, qui relie Ouest et Est.
pub const BLUE: Player = Player::P1;
/// Rouge, qui relie Nord et Sud.
pub const RED: Player = Player::P2;
/// Taille minimale du plateau.
pub const MIN_SIZE: usize = 2;
/// Taille maximale du plateau.
pub const MAX_SIZE: usize = 19;
/// Taille par défaut (cahier des charges : 14 × 14).
pub const DEFAULT_SIZE: usize = 14;

/// Case du plateau, d'indice `ligne × n + colonne`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Cell(pub u16);

impl Cell {
    /// Case à la ligne `row` et à la colonne `col` d'un plateau de taille `size`.
    #[must_use]
    pub fn from_rc(row: usize, col: usize, size: usize) -> Cell {
        Cell((row * size + col) as u16)
    }

    /// Ligne et colonne de la case sur un plateau de taille `size`.
    #[must_use]
    pub fn rc(self, size: usize) -> (usize, usize) {
        let i = usize::from(self.0);
        (i / size, i % size)
    }
}

/// Hex avec règle du gâteau (décorateur).
pub type HexSwap = SwapRule<HexState>;
/// Reverse Hex : relier ses deux bords fait perdre.
pub type ReverseHex = Misere<HexState>;
/// Reverse Hex avec règle du gâteau.
pub type ReverseHexSwap = SwapRule<Misere<HexState>>;

/// Politique de simulation de Reverse Hex.
pub type ReverseHexRollout = MisereRollout<HexRollout>;
/// Politique de simulation du Hex avec échange.
pub type HexSwapRollout = SwapRollout<HexState, HexRollout>;
/// Politique de simulation de Reverse Hex avec échange.
pub type ReverseHexSwapRollout = SwapRollout<Misere<HexState>, MisereRollout<HexRollout>>;
