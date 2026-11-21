//! Interfaces de jeu : [`Game`] (règles) et [`AmafGame`] (indexation des coups
//! pour l'heuristique « all-moves-as-first » de RAVE).

use std::fmt;

use crate::{GameError, Outcome, Player, rng::Rng};

/// Règles d'un jeu combinatoire à deux joueurs, à information parfaite.
///
/// # Contrat (vérifié par [`crate::contract`])
///
/// * `outcome()` est `None` si et seulement si `legal_moves` produit au moins
///   un coup ;
/// * jouer un coup renvoyé par `legal_moves` réussit toujours ;
/// * `play` sur une position terminée renvoie [`GameError::GameOver`] et ne
///   modifie pas l'état ;
/// * `play` ne panique jamais, quel que soit le coup fourni ;
/// * `ply` augmente de un à chaque coup joué ;
/// * le clonage (et `clone_from`) produit un état indiscernable de l'original ;
/// * `random_legal_move` renvoie un coup légal, ou `None` si la partie est finie.
pub trait Game: Clone {
    /// Type d'un coup.
    type Move: Copy + Eq + fmt::Debug;

    /// Joueur au trait.
    fn current_player(&self) -> Player;

    /// Remplit `out` (vidé au préalable) avec les coups légaux. Le tampon est
    /// fourni par l'appelant pour éviter toute allocation dans le chemin chaud.
    fn legal_moves(&self, out: &mut Vec<Self::Move>);

    /// Joue un coup pour le joueur au trait.
    ///
    /// # Errors
    ///
    /// Renvoie une [`GameError`] si le coup est illégal ou si la partie est finie.
    fn play(&mut self, mv: Self::Move) -> Result<(), GameError>;

    /// Résultat de la partie, ou `None` si elle n'est pas terminée.
    fn outcome(&self) -> Option<Outcome>;

    /// Nombre de coups joués depuis la position initiale.
    fn ply(&self) -> u32;

    /// Vrai si la partie est terminée.
    #[inline]
    fn is_terminal(&self) -> bool {
        self.outcome().is_some()
    }

    /// Coup légal tiré uniformément. L'implémentation par défaut énumère les
    /// coups légaux dans `buf` ; un jeu peut la spécialiser (O(1) au Hex).
    fn random_legal_move<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        buf: &mut Vec<Self::Move>,
    ) -> Option<Self::Move> {
        if self.is_terminal() {
            return None;
        }
        self.legal_moves(buf);
        if buf.is_empty() {
            None
        } else {
            Some(buf[rng.below(buf.len() as u32) as usize])
        }
    }
}

/// Jeu dont les coups ont un sens indépendamment de la position, ce qui permet
/// de partager leur valeur entre positions (heuristique AMAF, RAVE).
///
/// Un jeu comme le Quarto, dont les coups composés n'ont pas de sens hors
/// contexte, n'implémente pas ce trait (principe de ségrégation des interfaces).
///
/// # Contrat
///
/// * `move_index(mv) < move_index_bound()` pour tout coup légal ;
/// * deux coups légaux distincts ont des indices distincts ;
/// * l'indice d'un coup ne dépend pas de la position.
pub trait AmafGame: Game {
    /// Indice dense du coup, dans `[0, move_index_bound())`.
    fn move_index(&self, mv: Self::Move) -> usize;

    /// Taille de l'espace des indices de coups.
    fn move_index_bound(&self) -> usize;
}
