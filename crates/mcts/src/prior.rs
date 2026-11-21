//! Connaissance a priori : à la création d'un nœud, chaque action reçoit
//! une valeur heuristique H(s,a) et une confiance C(s,a) exprimée en simulations équivalentes (éq. 48-52).

use std::fmt;

use game_core::Game;

/// Valeur et confiance a priori d'une action.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prior {
    /// H(s, a) dans `[0, 1]`, du point de vue du joueur qui joue le coup.
    pub value: f32,
    /// C(s, a) : confiance pour la valeur Monte-Carlo.
    pub confidence: u32,
    /// C̃(s, a) : confiance pour la valeur AMAF.
    pub amaf_confidence: u32,
}

/// Source de connaissance a priori.
pub trait PriorKnowledge<G: Game>: fmt::Debug + Send {
    /// Prior de l'action `mv` dans l'état `state`, ou `None` pour aucun prior.
    fn prior(&self, state: &G, mv: G::Move) -> Option<Prior>;
}

/// Heuristique « partie équilibrée » de l'article : Q_even(s, a) = 0,5 avec
/// une confiance constante M.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvenGame {
    /// Confiance M (simulations équivalentes).
    pub confidence: u32,
}

impl<G: Game> PriorKnowledge<G> for EvenGame {
    fn prior(&self, _state: &G, _mv: G::Move) -> Option<Prior> {
        Some(Prior {
            value: 0.5,
            confidence: self.confidence,
            amaf_confidence: self.confidence,
        })
    }
}
