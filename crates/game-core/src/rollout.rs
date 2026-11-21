//! Politiques de simulation.
//!
//! Une politique joue une position jusqu'à la fin et peut enregistrer les coups
//! joués dans une [`Trace`], utilisée par RAVE pour les mises à jour AMAF.

use crate::{Game, Outcome, Player, rng::Rng};

/// Niveau d'enregistrement demandé à une politique de simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceMode {
    /// Aucun enregistrement.
    Off,
    /// Seul le premier coup est enregistré (attribué au nœud créé, UCT).
    FirstMove,
    /// Tous les coups, jusqu'au coup qui termine la partie (RAVE).
    Full,
}

/// Coups joués pendant une simulation, avec le joueur qui les a joués.
#[derive(Clone, Debug)]
pub struct Trace<M> {
    mode: TraceMode,
    entries: Vec<(M, Player)>,
}

impl<M: Copy> Trace<M> {
    /// Trace vide avec le mode indiqué.
    #[must_use]
    pub fn new(mode: TraceMode) -> Self {
        Trace {
            mode,
            entries: Vec::new(),
        }
    }

    /// Mode d'enregistrement courant.
    #[inline]
    #[must_use]
    pub fn mode(&self) -> TraceMode {
        self.mode
    }

    /// Change le mode d'enregistrement.
    #[inline]
    pub fn set_mode(&mut self, mode: TraceMode) {
        self.mode = mode;
    }

    /// Vide la trace en conservant sa capacité.
    #[inline]
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Enregistre un coup selon le mode courant.
    #[inline]
    pub fn record(&mut self, mv: M, player: Player) {
        match self.mode {
            TraceMode::Off => {}
            TraceMode::FirstMove => {
                if self.entries.is_empty() {
                    self.entries.push((mv, player));
                }
            }
            TraceMode::Full => self.entries.push((mv, player)),
        }
    }

    /// Enregistre une suite de coups selon le mode courant (plus rapide que
    /// des appels répétés à [`Trace::record`]).
    #[inline]
    pub fn extend<I>(&mut self, moves: I)
    where
        I: IntoIterator<Item = (M, Player)>,
    {
        match self.mode {
            TraceMode::Off => {}
            TraceMode::FirstMove => {
                if self.entries.is_empty() {
                    self.entries.extend(moves.into_iter().take(1));
                }
            }
            TraceMode::Full => self.entries.extend(moves),
        }
    }

    /// Coups enregistrés, dans l'ordre.
    #[inline]
    #[must_use]
    pub fn entries(&self) -> &[(M, Player)] {
        &self.entries
    }

    /// Nombre de coups enregistrés.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Vrai si aucun coup n'est enregistré.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Politique de simulation : joue `state` jusqu'à la fin de la partie.
///
/// # Contrat
///
/// * le résultat renvoyé est celui de la partie jouée ;
/// * en mode [`TraceMode::Full`], rejouer la trace depuis la position de départ
///   est légal et termine la partie exactement au dernier coup enregistré,
///   avec le même résultat ;
/// * après l'appel, `state` est dans un état valide mais non spécifié (une
///   politique optimisée peut ne pas y reporter les coups).
pub trait RolloutPolicy<G: Game> {
    /// Simule la fin de partie.
    fn rollout<R: Rng + ?Sized>(
        &mut self,
        state: &mut G,
        rng: &mut R,
        trace: &mut Trace<G::Move>,
    ) -> Outcome;
}

/// Politique uniforme générique : à chaque tour, un coup légal tiré au hasard.
#[derive(Clone, Debug)]
pub struct UniformRollout<G: Game> {
    buf: Vec<G::Move>,
}

impl<G: Game> Default for UniformRollout<G> {
    fn default() -> Self {
        UniformRollout { buf: Vec::new() }
    }
}

impl<G: Game> UniformRollout<G> {
    /// Nouvelle politique uniforme.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl<G: Game> RolloutPolicy<G> for UniformRollout<G> {
    fn rollout<R: Rng + ?Sized>(
        &mut self,
        state: &mut G,
        rng: &mut R,
        trace: &mut Trace<G::Move>,
    ) -> Outcome {
        loop {
            if let Some(outcome) = state.outcome() {
                return outcome;
            }
            let player = state.current_player();
            let Some(mv) = state.random_legal_move(rng, &mut self.buf) else {
                // Violation du contrat de Game (position non terminale sans coup) :
                // on termine prudemment par un nul plutôt que de paniquer.
                debug_assert!(false, "position non terminale sans coup légal");
                return Outcome::Draw;
            };
            if state.play(mv).is_err() {
                debug_assert!(false, "random_legal_move a renvoyé un coup illégal");
                return Outcome::Draw;
            }
            trace.record(mv, player);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_modes() {
        let mut t: Trace<u8> = Trace::new(TraceMode::Off);
        t.record(1, Player::P1);
        assert!(t.is_empty());
        t.set_mode(TraceMode::FirstMove);
        t.record(1, Player::P1);
        t.record(2, Player::P2);
        assert_eq!(t.entries(), &[(1, Player::P1)]);
        t.clear();
        t.set_mode(TraceMode::Full);
        t.record(1, Player::P1);
        t.record(2, Player::P2);
        assert_eq!(t.len(), 2);
    }
}
