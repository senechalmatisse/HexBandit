//! Registre des jeux et fabrique des joueurs :
//! le seul endroit où les types concrets sont assemblés (inversion des dépendances).
//! Ajouter une variante de jeu = implémenter [`EngineGame`] et
//! ajouter une ligne à [`dispatch`] ; le crate `mcts` n'est pas modifié.

use game_chameleon::{
    ChameleonState, format_move as format_chameleon, parse_move as parse_chameleon,
};
use game_core::{
    AmafGame, Game, GameError, Player,
    decorators::{Misere, MisereRollout, SwapMove, SwapRollout, SwapRule},
    rng::Pcg32,
    rollout::{RolloutPolicy, UniformRollout},
};
use game_hex::{
    Cell, HexRollout, HexState, ReverseHex,
    notation::{format_cell, parse_cell},
    patterns,
};
use game_quarto::QuartoState;
use mcts::{
    Agent, Mcts, MctsAgent, MctsBuilder, RandomAgent, Rave, ReportDetail, RootParallelAgent,
    TreeAlgorithm, Uct,
    beta::{AnySchedule, HandSelected, MinimumMse, NoRave},
    budget::{Budget, SimulationBudget, TimeBudget},
    final_move::{BestValue, FinalMoveStrategy, MaxRobust, MostVisited, SecureChild},
    prior::{EvenGame, Prior, PriorKnowledge},
    runner::player_rng,
};

use crate::{
    config::{
        AlgorithmSpec, BetaSpec, BudgetSpec, FinalMoveSpec, GameKind, GameSpec, HexPriorWeights,
        PlayerSpec, PriorSpec, RolloutSpec,
    },
    error::BindingError,
};

/// Jeu pilotable par la façade : règles, politique de simulation et notation.
pub(crate) trait EngineGame: Game + Send + Sync + 'static
where
    Self::Move: Send + Sync,
{
    /// Politique de simulation associée.
    type Rollout: RolloutPolicy<Self> + Send + 'static;

    /// Position initiale.
    fn initial(spec: &GameSpec, first: Player) -> Result<Self, GameError>;

    /// Position décrite textuellement (sans historique).
    fn from_position(spec: &GameSpec, text: &str) -> Result<Self, GameError>;

    /// Politique de simulation demandée.
    fn rollout(kind: RolloutSpec) -> Self::Rollout;

    /// Notation d'un coup.
    fn format_move(&self, mv: Self::Move) -> String;

    /// Lecture d'un coup.
    fn parse_move(&self, text: &str) -> Result<Self::Move, GameError>;

    /// Position textuelle (pour les rapports de recherche).
    fn position(&self) -> String;

    /// Joueur MC-RAVE, si le jeu le permet (il faut `AmafGame`). Un jeu dont
    /// les coups n'ont pas de sens hors contexte renvoie une erreur.
    fn rave_agent(
        spec: &PlayerSpec,
        rng: Pcg32,
        label: String,
        detail: ReportDetail,
    ) -> Result<Box<dyn Agent<Self>>, BindingError>;

    /// Connaissance a priori fondée sur les motifs du Hex, si elle a un sens pour ce jeu.
    fn hex_prior(_weights: HexPriorWeights) -> Option<Box<dyn PriorKnowledge<Self>>> {
        None
    }

    /// Même chose pour ce jeu muni de la règle du gâteau.
    fn swap_hex_prior(_weights: HexPriorWeights) -> Option<Box<dyn PriorKnowledge<SwapRule<Self>>>>
    where
        Self: game_core::decorators::PieGame,
    {
        None
    }
}

/// Connaissance a priori demandée par la configuration.
fn prior_for<G: EngineGame>(
    spec: &PlayerSpec,
) -> Result<Option<Box<dyn PriorKnowledge<G>>>, BindingError>
where
    G::Move: Send + Sync,
{
    match spec.prior {
        None => Ok(None),
        Some(PriorSpec::Even { confidence }) => Ok(Some(Box::new(EvenGame { confidence }))),
        Some(PriorSpec::Hex(weights)) => G::hex_prior(weights).map(Some).ok_or_else(|| {
            BindingError::config(
                "prior « hex » disponible seulement pour le Hex (avec ou sans règle du gâteau)",
            )
        }),
    }
}

/// Joueur MCTS : un moteur, ou `trees` moteurs en parallèle à la racine, assemblés avec les options de la configuration.
fn mcts_agent<G, A>(
    spec: &PlayerSpec,
    make_algorithm: impl Fn() -> A,
    rng: Pcg32,
    label: String,
    detail: ReportDetail,
) -> Result<Box<dyn Agent<G>>, BindingError>
where
    G: EngineGame,
    G::Move: Send + Sync,
    A: TreeAlgorithm<G> + 'static,
{
    let build = || -> Result<Mcts<G, A, G::Rollout>, BindingError> {
        let mut builder = MctsBuilder::new(make_algorithm(), G::rollout(spec.rollout))
            .expand_threshold(spec.expand_threshold)
            .max_nodes(spec.max_nodes)
            .detail(detail)
            .final_move(final_move(spec))
            .solver(spec.solver)
            .reuse_tree(spec.reuse_tree);
        if let Some(prior) = prior_for::<G>(spec)? {
            builder = builder.prior(prior);
        }
        Ok(builder.build()?)
    };
    if spec.trees > 1 {
        let workers = (0..spec.trees)
            .map(|_| build())
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Box::new(RootParallelAgent::new(
            workers,
            budget(spec),
            rng,
            label,
        )))
    } else {
        Ok(Box::new(MctsAgent::new(build()?, budget(spec), rng, label)))
    }
}

/// Joueur MC-RAVE (ou GRAVE) pour un jeu qui implémente `AmafGame`.
fn rave_for<G: EngineGame + AmafGame>(
    spec: &PlayerSpec,
    rng: Pcg32,
    label: String,
    detail: ReportDetail,
) -> Result<Box<dyn Agent<G>>, BindingError>
where
    G::Move: Send + Sync,
{
    mcts_agent(
        spec,
        || Rave::new(schedule(spec.beta), spec.exploration).with_grave(spec.grave_ref),
        rng,
        label,
        detail,
    )
}

/// Connaissance a priori du Hex : valeur et confiance de chaque
/// coup d'après les motifs locaux de [`game_hex::patterns`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct HexPrior {
    weights: HexPriorWeights,
}

impl HexPrior {
    fn evaluate(&self, state: &HexState, cell: Cell) -> Option<Prior> {
        let w = self.weights;
        let f = patterns::move_features(state, cell);
        let (value, confidence) = if f.dead {
            (0.0, w.dead)
        } else if f.saves > 0 {
            (1.0, w.save)
        } else if f.cuts > 0 {
            (0.9, w.cut)
        } else if f.bridges > 0 || f.edge_template {
            (0.75, w.bridge)
        } else {
            (0.5, w.even)
        };
        (confidence > 0).then_some(Prior {
            value,
            confidence,
            amaf_confidence: confidence,
        })
    }
}

impl PriorKnowledge<HexState> for HexPrior {
    fn prior(&self, state: &HexState, mv: Cell) -> Option<Prior> {
        self.evaluate(state, mv)
    }
}

impl PriorKnowledge<SwapRule<HexState>> for HexPrior {
    fn prior(&self, state: &SwapRule<HexState>, mv: SwapMove<Cell>) -> Option<Prior> {
        match mv {
            SwapMove::Place(cell) => self.evaluate(state.inner(), cell),
            SwapMove::Swap => None,
        }
    }
}

fn hex_initial(spec: &GameSpec, first: Player) -> Result<HexState, GameError> {
    HexState::with_first_player(spec.size, first)
}

fn hex_from_position(spec: &GameSpec, text: &str) -> Result<HexState, GameError> {
    let state = HexState::from_position(text)?;
    if state.size() != spec.size {
        return Err(GameError::Parse {
            input: text.to_owned(),
            reason: format!(
                "taille {} différente de game.size = {}",
                state.size(),
                spec.size
            ),
        });
    }
    Ok(state)
}

fn hex_rollout(kind: RolloutSpec) -> HexRollout {
    match kind {
        RolloutSpec::Fill => HexRollout::fill(),
        RolloutSpec::Uniform => HexRollout::uniform(),
    }
}

impl EngineGame for HexState {
    type Rollout = HexRollout;

    fn initial(spec: &GameSpec, first: Player) -> Result<Self, GameError> {
        hex_initial(spec, first)
    }

    fn from_position(spec: &GameSpec, text: &str) -> Result<Self, GameError> {
        hex_from_position(spec, text)
    }

    fn rollout(kind: RolloutSpec) -> Self::Rollout {
        hex_rollout(kind)
    }

    fn format_move(&self, mv: Self::Move) -> String {
        format_cell(mv, self.size())
    }

    fn parse_move(&self, text: &str) -> Result<Self::Move, GameError> {
        parse_cell(text, self.size())
    }

    fn position(&self) -> String {
        self.to_position()
    }

    fn rave_agent(
        spec: &PlayerSpec,
        rng: Pcg32,
        label: String,
        detail: ReportDetail,
    ) -> Result<Box<dyn Agent<Self>>, BindingError> {
        rave_for::<Self>(spec, rng, label, detail)
    }

    fn hex_prior(weights: HexPriorWeights) -> Option<Box<dyn PriorKnowledge<Self>>> {
        Some(Box::new(HexPrior { weights }))
    }

    fn swap_hex_prior(weights: HexPriorWeights) -> Option<Box<dyn PriorKnowledge<SwapRule<Self>>>> {
        Some(Box::new(HexPrior { weights }))
    }
}

impl EngineGame for ReverseHex {
    type Rollout = MisereRollout<HexRollout>;

    fn initial(spec: &GameSpec, first: Player) -> Result<Self, GameError> {
        hex_initial(spec, first).map(Misere::new)
    }

    fn from_position(spec: &GameSpec, text: &str) -> Result<Self, GameError> {
        hex_from_position(spec, text).map(Misere::new)
    }

    fn rollout(kind: RolloutSpec) -> Self::Rollout {
        MisereRollout::new(hex_rollout(kind))
    }

    fn format_move(&self, mv: Self::Move) -> String {
        self.inner().format_move(mv)
    }

    fn parse_move(&self, text: &str) -> Result<Self::Move, GameError> {
        self.inner().parse_move(text)
    }

    fn position(&self) -> String {
        self.inner().to_position()
    }

    fn rave_agent(
        spec: &PlayerSpec,
        rng: Pcg32,
        label: String,
        detail: ReportDetail,
    ) -> Result<Box<dyn Agent<Self>>, BindingError> {
        rave_for::<Self>(spec, rng, label, detail)
    }
}

impl<G> EngineGame for SwapRule<G>
where
    G: EngineGame + AmafGame + game_core::decorators::PieGame,
    G::Move: Send + Sync,
{
    type Rollout = SwapRollout<G, G::Rollout>;

    fn initial(spec: &GameSpec, first: Player) -> Result<Self, GameError> {
        G::initial(spec, first).map(SwapRule::new)
    }

    fn from_position(spec: &GameSpec, text: &str) -> Result<Self, GameError> {
        G::from_position(spec, text).map(SwapRule::new)
    }

    fn rollout(kind: RolloutSpec) -> Self::Rollout {
        SwapRollout::new(G::rollout(kind))
    }

    fn format_move(&self, mv: Self::Move) -> String {
        match mv {
            SwapMove::Swap => "swap".to_owned(),
            SwapMove::Place(m) => self.inner().format_move(m),
        }
    }

    fn parse_move(&self, text: &str) -> Result<Self::Move, GameError> {
        if text.trim().eq_ignore_ascii_case("swap") {
            Ok(SwapMove::Swap)
        } else {
            self.inner().parse_move(text).map(SwapMove::Place)
        }
    }

    fn position(&self) -> String {
        self.inner().position()
    }

    fn rave_agent(
        spec: &PlayerSpec,
        rng: Pcg32,
        label: String,
        detail: ReportDetail,
    ) -> Result<Box<dyn Agent<Self>>, BindingError> {
        rave_for::<Self>(spec, rng, label, detail)
    }

    fn hex_prior(weights: HexPriorWeights) -> Option<Box<dyn PriorKnowledge<Self>>> {
        G::swap_hex_prior(weights)
    }
}

impl EngineGame for QuartoState {
    type Rollout = UniformRollout<QuartoState>;

    fn initial(spec: &GameSpec, first: Player) -> Result<Self, GameError> {
        Ok(QuartoState::new(
            first,
            spec.kind == GameKind::QuartoSquares,
        ))
    }

    fn from_position(_spec: &GameSpec, text: &str) -> Result<Self, GameError> {
        Err(GameError::Parse {
            input: text.to_owned(),
            reason: "positions textuelles non prises en charge au Quarto : donner les coups"
                .to_owned(),
        })
    }

    fn rollout(_kind: RolloutSpec) -> Self::Rollout {
        // La politique de remplissage est propre au Hex : le Quarto joue
        // toujours des coups uniformes.
        UniformRollout::new()
    }

    fn format_move(&self, mv: Self::Move) -> String {
        game_quarto::format_move(mv)
    }

    fn parse_move(&self, text: &str) -> Result<Self::Move, GameError> {
        game_quarto::parse_move(text)
    }

    fn position(&self) -> String {
        let cells: String = (0..16)
            .map(|c| {
                self.piece_at(c)
                    .map_or('.', |p| char::from_digit(u32::from(p), 16).unwrap_or('?'))
            })
            .collect();
        match self.in_hand() {
            Some(p) => format!("{cells} {p:x}"),
            None => cells,
        }
    }

    fn rave_agent(
        _spec: &PlayerSpec,
        _rng: Pcg32,
        _label: String,
        _detail: ReportDetail,
    ) -> Result<Box<dyn Agent<Self>>, BindingError> {
        Err(BindingError::config(
            "MC-RAVE indisponible pour le Quarto : ses coups n'ont pas de sens hors contexte \
             (le jeu n'implémente pas AmafGame) ; utiliser uct",
        ))
    }
}

impl EngineGame for ChameleonState {
    type Rollout = UniformRollout<ChameleonState>;

    fn initial(spec: &GameSpec, first: Player) -> Result<Self, GameError> {
        ChameleonState::with_first_player(spec.size, first)
    }

    fn from_position(spec: &GameSpec, text: &str) -> Result<Self, GameError> {
        let state = ChameleonState::from_position(text)?;
        if state.size() != spec.size {
            return Err(GameError::Parse {
                input: text.to_owned(),
                reason: format!(
                    "taille {} différente de game.size = {}",
                    state.size(),
                    spec.size
                ),
            });
        }
        Ok(state)
    }

    fn rollout(_kind: RolloutSpec) -> Self::Rollout {
        // Le remplissage aléatoire ne s'applique pas : la partie s'arrête à
        // la première chaîne et son vainqueur dépend de l'ordre des coups.
        UniformRollout::new()
    }

    fn format_move(&self, mv: Self::Move) -> String {
        format_chameleon(mv, self.size())
    }

    fn parse_move(&self, text: &str) -> Result<Self::Move, GameError> {
        parse_chameleon(text, self.size())
    }

    fn position(&self) -> String {
        self.to_position()
    }

    fn rave_agent(
        spec: &PlayerSpec,
        rng: Pcg32,
        label: String,
        detail: ReportDetail,
    ) -> Result<Box<dyn Agent<Self>>, BindingError> {
        // Chameleon implémente AmafGame (indice case × 2 + couleur) : MC-RAVE
        // s'applique tel quel, sans modifier le crate mcts.
        rave_for::<Self>(spec, rng, label, detail)
    }
}

/// Visiteur générique : code exécuté pour le type de jeu concret choisi.
pub(crate) trait GameVisitor {
    /// Résultat.
    type Output;
    /// Exécution pour le jeu `G`.
    fn visit<G: EngineGame>(self, spec: &GameSpec) -> Self::Output
    where
        G::Move: Send + Sync;
}

/// Registre : choisit le type concret de jeu à partir de la configuration.
pub(crate) fn dispatch<V: GameVisitor>(spec: &GameSpec, visitor: V) -> V::Output {
    match (spec.kind, spec.swap) {
        (GameKind::Hex, false) => visitor.visit::<HexState>(spec),
        (GameKind::Hex, true) => visitor.visit::<SwapRule<HexState>>(spec),
        (GameKind::ReverseHex, false) => visitor.visit::<ReverseHex>(spec),
        (GameKind::ReverseHex, true) => visitor.visit::<SwapRule<ReverseHex>>(spec),
        (GameKind::Quarto | GameKind::QuartoSquares, _) => visitor.visit::<QuartoState>(spec),
        (GameKind::Chameleon, false) => visitor.visit::<ChameleonState>(spec),
        (GameKind::Chameleon, true) => visitor.visit::<SwapRule<ChameleonState>>(spec),
    }
}

fn budget(spec: &PlayerSpec) -> Box<dyn Budget> {
    match spec.budget {
        BudgetSpec::Simulations(simulations) => Box::new(SimulationBudget { simulations }),
        BudgetSpec::TimeMs(ms) => Box::new(TimeBudget::new(std::time::Duration::from_millis(ms))),
    }
}

fn final_move(spec: &PlayerSpec) -> Box<dyn FinalMoveStrategy> {
    match spec.final_move {
        FinalMoveSpec::MostVisited => Box::new(MostVisited),
        FinalMoveSpec::BestValue => Box::new(BestValue),
        FinalMoveSpec::Secure { a } => Box::new(SecureChild { a }),
        FinalMoveSpec::MaxRobust => Box::new(MaxRobust),
    }
}

fn schedule(beta: BetaSpec) -> AnySchedule {
    match beta {
        BetaSpec::HandSelected { k } => AnySchedule::HandSelected(HandSelected { k }),
        BetaSpec::MinimumMse { bias } => AnySchedule::MinimumMse(MinimumMse { bias }),
        BetaSpec::None => AnySchedule::None(NoRave),
    }
}

/// Fabrique un joueur pour le jeu `G`.
pub(crate) fn build_agent<G: EngineGame>(
    spec: &PlayerSpec,
    game_seed: u64,
    color: Player,
    detail: ReportDetail,
) -> Result<Box<dyn Agent<G>>, BindingError>
where
    G::Move: Send + Sync,
{
    let rng = player_rng(game_seed, color);
    let label = spec.short_label();
    match spec.algorithm {
        AlgorithmSpec::Random => Ok(Box::new(RandomAgent::<G>::new(rng))),
        AlgorithmSpec::Uct => mcts_agent(
            spec,
            || Uct {
                exploration: spec.exploration,
            },
            rng,
            label,
            detail,
        ),
        AlgorithmSpec::Rave => G::rave_agent(spec, rng, label, detail),
    }
}
