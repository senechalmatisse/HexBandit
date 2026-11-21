//! Construction validée du moteur.

use game_core::{Game, rollout::RolloutPolicy};

use crate::{
    algorithm::TreeAlgorithm,
    final_move::{FinalMoveStrategy, MostVisited},
    prior::PriorKnowledge,
    report::{ReportDetail, SearchError},
    search::{Mcts, SearchConfig},
};

/// Constructeur de [`Mcts`].
///
/// ```
/// use game_hex::{HexRollout, HexState};
/// use mcts::{MctsBuilder, Uct};
///
/// let engine = MctsBuilder::<HexState, _, _>::new(Uct::default(), HexRollout::fill())
///     .max_nodes(10_000)
///     .build()
///     .unwrap();
/// assert_eq!(engine.config().max_nodes, 10_000);
/// ```
pub struct MctsBuilder<G: Game, A, P> {
    config: SearchConfig,
    algorithm: A,
    rollout: P,
    final_move: Box<dyn FinalMoveStrategy>,
    prior: Option<Box<dyn PriorKnowledge<G>>>,
}

impl<G, A, P> MctsBuilder<G, A, P>
where
    G: Game,
    A: TreeAlgorithm<G>,
    P: RolloutPolicy<G>,
{
    /// Constructeur avec les paramètres par défaut.
    #[must_use]
    pub fn new(algorithm: A, rollout: P) -> Self {
        MctsBuilder {
            config: SearchConfig::default(),
            algorithm,
            rollout,
            final_move: Box::new(MostVisited),
            prior: None,
        }
    }

    /// Passages par une arête avant expansion (≥ 1).
    #[must_use]
    pub fn expand_threshold(mut self, threshold: u32) -> Self {
        self.config.expand_threshold = threshold;
        self
    }

    /// Nombre maximal de nœuds (≥ 1).
    #[must_use]
    pub fn max_nodes(mut self, max_nodes: u32) -> Self {
        self.config.max_nodes = max_nodes;
        self
    }

    /// Niveau de détail du rapport.
    #[must_use]
    pub fn detail(mut self, detail: ReportDetail) -> Self {
        self.config.detail = detail;
        self
    }

    /// MCTS-Solver : propagation des positions prouvées (désactivé par défaut).
    #[must_use]
    pub fn solver(mut self, enabled: bool) -> Self {
        self.config.solver = enabled;
        self
    }

    /// Réutilisation du sous-arbre du coup joué (désactivée par défaut).
    #[must_use]
    pub fn reuse_tree(mut self, enabled: bool) -> Self {
        self.config.reuse_tree = enabled;
        self
    }

    /// Stratégie de choix final.
    #[must_use]
    pub fn final_move(mut self, strategy: Box<dyn FinalMoveStrategy>) -> Self {
        self.final_move = strategy;
        self
    }

    /// Connaissance a priori.
    #[must_use]
    pub fn prior(mut self, prior: Box<dyn PriorKnowledge<G>>) -> Self {
        self.prior = Some(prior);
        self
    }

    /// Construit le moteur.
    ///
    /// # Errors
    ///
    /// [`SearchError::InvalidConfig`] si la configuration est incohérente.
    pub fn build(self) -> Result<Mcts<G, A, P>, SearchError> {
        self.config.validate()?;
        let mut mcts = Mcts::new(self.config, self.algorithm, self.rollout);
        mcts.set_final_move(self.final_move);
        mcts.set_prior(self.prior);
        Ok(mcts)
    }
}
