//! Décorateurs de jeux (pattern Decorator) : ajoutent une règle à un jeu sans modifier son type.
//!
//! * [`Misere`] inverse le résultat (Reverse Hex : relier ses bords fait perdre) ;
//! * [`SwapRule`] ajoute la règle du gâteau (« pie rule ») : après le premier
//!   coup, le second joueur peut échanger les rôles.
//!
//! Chaque décorateur a sa politique de simulation associée, qui délègue à la politique du jeu enveloppé.

use crate::{
    AmafGame, Game, GameError, Outcome, Player,
    rng::Rng,
    rollout::{RolloutPolicy, Trace, TraceMode},
};

// ---------------------------------------------------------------------------
// Misère
// ---------------------------------------------------------------------------

/// Variante « misère » d'un jeu : le joueur qui gagnerait au jeu normal perd.
#[derive(Debug, PartialEq, Eq)]
pub struct Misere<G> {
    inner: G,
}

impl<G> Misere<G> {
    /// Enveloppe un jeu.
    #[must_use]
    pub fn new(inner: G) -> Self {
        Misere { inner }
    }

    /// Jeu enveloppé.
    #[must_use]
    pub fn inner(&self) -> &G {
        &self.inner
    }

    /// Jeu enveloppé, mutable.
    pub fn inner_mut(&mut self) -> &mut G {
        &mut self.inner
    }

    /// Récupère le jeu enveloppé.
    #[must_use]
    pub fn into_inner(self) -> G {
        self.inner
    }
}

impl<G: Clone> Clone for Misere<G> {
    fn clone(&self) -> Self {
        Misere {
            inner: self.inner.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.inner.clone_from(&source.inner);
    }
}

impl<G: Game> Game for Misere<G> {
    type Move = G::Move;

    #[inline]
    fn current_player(&self) -> Player {
        self.inner.current_player()
    }

    #[inline]
    fn legal_moves(&self, out: &mut Vec<Self::Move>) {
        self.inner.legal_moves(out);
    }

    #[inline]
    fn play(&mut self, mv: Self::Move) -> Result<(), GameError> {
        self.inner.play(mv)
    }

    #[inline]
    fn outcome(&self) -> Option<Outcome> {
        self.inner.outcome().map(Outcome::inverted)
    }

    #[inline]
    fn ply(&self) -> u32 {
        self.inner.ply()
    }

    #[inline]
    fn random_legal_move<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        buf: &mut Vec<Self::Move>,
    ) -> Option<Self::Move> {
        self.inner.random_legal_move(rng, buf)
    }
}

impl<G: AmafGame> AmafGame for Misere<G> {
    #[inline]
    fn move_index(&self, mv: Self::Move) -> usize {
        self.inner.move_index(mv)
    }

    #[inline]
    fn move_index_bound(&self) -> usize {
        self.inner.move_index_bound()
    }
}

/// Politique de simulation d'un jeu misère : simule le jeu enveloppé et
/// inverse le résultat. La trace est identique (la partie s'arrête au même coup).
#[derive(Clone, Debug, Default)]
pub struct MisereRollout<P> {
    inner: P,
}

impl<P> MisereRollout<P> {
    /// Enveloppe une politique du jeu normal.
    #[must_use]
    pub fn new(inner: P) -> Self {
        MisereRollout { inner }
    }
}

impl<G: Game, P: RolloutPolicy<G>> RolloutPolicy<Misere<G>> for MisereRollout<P> {
    #[inline]
    fn rollout<R: Rng + ?Sized>(
        &mut self,
        state: &mut Misere<G>,
        rng: &mut R,
        trace: &mut Trace<G::Move>,
    ) -> Outcome {
        self.inner.rollout(state.inner_mut(), rng, trace).inverted()
    }
}

// ---------------------------------------------------------------------------
// Règle du gâteau (swap)
// ---------------------------------------------------------------------------

/// Jeu qui sait appliquer l'échange de la règle du gâteau.
pub trait PieGame: Game {
    /// Applique l'échange sur une position où exactement un coup a été joué :
    /// le second joueur prend à son compte le premier coup, puis c'est de
    /// nouveau au premier joueur de jouer.
    ///
    /// # Errors
    ///
    /// Renvoie [`GameError::IllegalMove`] si la position ne s'y prête pas.
    fn apply_swap(&mut self) -> Result<(), GameError>;
}

impl<G: PieGame> PieGame for Misere<G> {
    fn apply_swap(&mut self) -> Result<(), GameError> {
        self.inner.apply_swap()
    }
}

/// Coup d'un jeu muni de la règle du gâteau.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SwapMove<M> {
    /// Coup ordinaire du jeu enveloppé.
    Place(M),
    /// Échange (seulement au deuxième coup de la partie).
    Swap,
}

/// Décorateur ajoutant la règle du gâteau.
#[derive(Debug, PartialEq, Eq)]
pub struct SwapRule<G> {
    inner: G,
    swapped: bool,
}

impl<G: Clone> Clone for SwapRule<G> {
    fn clone(&self) -> Self {
        SwapRule {
            inner: self.inner.clone(),
            swapped: self.swapped,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.inner.clone_from(&source.inner);
        self.swapped = source.swapped;
    }
}

impl<G: Game> SwapRule<G> {
    /// Enveloppe un jeu (dont aucun coup ne doit encore avoir été joué).
    #[must_use]
    pub fn new(inner: G) -> Self {
        SwapRule {
            inner,
            swapped: false,
        }
    }

    /// Jeu enveloppé.
    #[must_use]
    pub fn inner(&self) -> &G {
        &self.inner
    }

    /// Vrai si l'échange a eu lieu.
    #[must_use]
    pub fn swapped(&self) -> bool {
        self.swapped
    }

    /// Vrai si l'échange est légal maintenant (juste après le premier coup).
    #[inline]
    #[must_use]
    pub fn swap_available(&self) -> bool {
        !self.swapped && self.inner.ply() == 1 && !self.inner.is_terminal()
    }
}

impl<G: PieGame> Game for SwapRule<G> {
    type Move = SwapMove<G::Move>;

    #[inline]
    fn current_player(&self) -> Player {
        self.inner.current_player()
    }

    fn legal_moves(&self, out: &mut Vec<Self::Move>) {
        out.clear();
        let mut inner_moves = Vec::new();
        self.inner.legal_moves(&mut inner_moves);
        out.extend(inner_moves.into_iter().map(SwapMove::Place));
        if self.swap_available() {
            out.push(SwapMove::Swap);
        }
    }

    fn play(&mut self, mv: Self::Move) -> Result<(), GameError> {
        match mv {
            SwapMove::Place(m) => self.inner.play(m),
            SwapMove::Swap => {
                if self.inner.is_terminal() {
                    return Err(GameError::GameOver);
                }
                if !self.swap_available() {
                    return Err(GameError::IllegalMove {
                        mv: "swap".to_owned(),
                        reason: "l'échange n'est possible qu'au deuxième coup".to_owned(),
                    });
                }
                self.inner.apply_swap()?;
                self.swapped = true;
                Ok(())
            }
        }
    }

    #[inline]
    fn outcome(&self) -> Option<Outcome> {
        self.inner.outcome()
    }

    #[inline]
    fn ply(&self) -> u32 {
        self.inner.ply()
    }

    fn random_legal_move<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        buf: &mut Vec<Self::Move>,
    ) -> Option<Self::Move> {
        if self.swap_available() {
            self.legal_moves(buf);
            if buf.is_empty() {
                return None;
            }
            return Some(buf[rng.below(buf.len() as u32) as usize]);
        }
        let mut inner_buf = Vec::new();
        self.inner
            .random_legal_move(rng, &mut inner_buf)
            .map(SwapMove::Place)
    }
}

impl<G: PieGame + AmafGame> AmafGame for SwapRule<G> {
    #[inline]
    fn move_index(&self, mv: Self::Move) -> usize {
        match mv {
            SwapMove::Place(m) => self.inner.move_index(m),
            SwapMove::Swap => self.inner.move_index_bound(),
        }
    }

    #[inline]
    fn move_index_bound(&self) -> usize {
        self.inner.move_index_bound() + 1
    }
}

/// Politique de simulation avec règle du gâteau : décide uniformément de
/// l'échange s'il est possible, puis délègue à la politique du jeu enveloppé.
#[derive(Clone, Debug)]
pub struct SwapRollout<G: Game, P> {
    inner: P,
    inner_trace: Trace<G::Move>,
    buf: Vec<SwapMove<G::Move>>,
}

impl<G: Game, P> SwapRollout<G, P> {
    /// Enveloppe une politique du jeu sans échange.
    #[must_use]
    pub fn new(inner: P) -> Self {
        SwapRollout {
            inner,
            inner_trace: Trace::new(TraceMode::Off),
            buf: Vec::new(),
        }
    }
}

impl<G: PieGame, P: RolloutPolicy<G>> RolloutPolicy<SwapRule<G>> for SwapRollout<G, P> {
    fn rollout<R: Rng + ?Sized>(
        &mut self,
        state: &mut SwapRule<G>,
        rng: &mut R,
        trace: &mut Trace<SwapMove<G::Move>>,
    ) -> Outcome {
        if state.swap_available() {
            let player = state.current_player();
            if let Some(mv) = state.random_legal_move(rng, &mut self.buf) {
                if state.play(mv).is_ok() {
                    trace.record(mv, player);
                }
            }
        }
        let inner_mode = match trace.mode() {
            TraceMode::FirstMove if !trace.is_empty() => TraceMode::Off,
            mode => mode,
        };
        self.inner_trace.set_mode(inner_mode);
        self.inner_trace.clear();
        let outcome = self
            .inner
            .rollout(&mut state.inner, rng, &mut self.inner_trace);
        for &(mv, player) in self.inner_trace.entries() {
            trace.record(SwapMove::Place(mv), player);
        }
        outcome
    }
}
