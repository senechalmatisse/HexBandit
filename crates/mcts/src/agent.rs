//! Joueurs artificiels : un joueur choisit un coup dans une position.
//!
//! * [`MctsAgent`] : un moteur, un budget par coup et un générateur propre ;
//! * [`RootParallelAgent`] : parallélisme à la racine :
//!   plusieurs arbres indépendants, chacun avec sa part du budget et son flux
//!   aléatoire, dont les statistiques de la racine sont fusionnées. Le
//!   résultat ne dépend que du nombre d'arbres, pas de l'ordonnancement des
//!   threads : il reste déterministe ;
//! * [`RandomAgent`] : coups uniformes (référence).

use game_core::{
    Game,
    rng::{Pcg32, Rng, derive_seed},
    rollout::RolloutPolicy,
};

use crate::{
    algorithm::TreeAlgorithm,
    budget::Budget,
    cancel::CancelToken,
    final_move::CandidateSummary,
    report::{CandidateStats, ReportDetail, SearchError, SearchReport},
    search::Mcts,
    tree::{AmafStats, Edge, NO_NODE, Node, Proof},
};

/// Joueur artificiel.
pub trait Agent<G: Game>: Send {
    /// Nom lisible (algorithme et budget).
    fn label(&self) -> &str;

    /// Choisit un coup ; le rapport contient le coup retenu (`best_move`).
    ///
    /// # Errors
    ///
    /// Erreur de recherche (position terminale, annulation…).
    fn choose(
        &mut self,
        state: &G,
        cancel: &CancelToken,
    ) -> Result<SearchReport<G::Move>, SearchError>;

    /// Un coup vient d'être joué dans la partie (par ce joueur ou par son
    /// adversaire). Un joueur qui réutilise son arbre s'en sert pour
    /// retrouver la nouvelle position dans l'arbre ; par défaut, rien.
    fn observe(&mut self, _mv: G::Move) {}
}

/// Joueur MCTS : moteur, budget par coup et générateur aléatoire propre.
pub struct MctsAgent<G: Game, A: TreeAlgorithm<G>, P: RolloutPolicy<G>> {
    mcts: Mcts<G, A, P>,
    budget: Box<dyn Budget>,
    rng: Pcg32,
    label: String,
    /// Coups joués depuis la racine de la dernière recherche.
    played: Vec<G::Move>,
}

impl<G, A, P> MctsAgent<G, A, P>
where
    G: Game,
    A: TreeAlgorithm<G>,
    P: RolloutPolicy<G>,
{
    /// Assemble un joueur.
    #[must_use]
    pub fn new(mcts: Mcts<G, A, P>, budget: Box<dyn Budget>, rng: Pcg32, label: String) -> Self {
        MctsAgent {
            mcts,
            budget,
            rng,
            label,
            played: Vec::new(),
        }
    }

    /// Moteur du joueur.
    #[must_use]
    pub fn engine(&self) -> &Mcts<G, A, P> {
        &self.mcts
    }
}

impl<G, A, P> Agent<G> for MctsAgent<G, A, P>
where
    G: Game + Send,
    G::Move: Send,
    A: TreeAlgorithm<G>,
    P: RolloutPolicy<G> + Send,
{
    fn label(&self) -> &str {
        &self.label
    }

    fn choose(
        &mut self,
        state: &G,
        cancel: &CancelToken,
    ) -> Result<SearchReport<G::Move>, SearchError> {
        let report = self.mcts.search_after(
            state,
            &self.played,
            &mut *self.budget,
            &mut self.rng,
            cancel,
        );
        self.played.clear();
        report
    }

    fn observe(&mut self, mv: G::Move) {
        self.played.push(mv);
    }
}

/// Parallélisme à la racine : `workers.len()` arbres indépendants.
///
/// Chaque arbre reçoit sa part du budget ([`Budget::share`]) et son propre
/// générateur, dérivé de celui du joueur ; les recherches tournent chacune
/// dans un thread. Les statistiques des coups de la racine sont ensuite
/// fusionnées dans l'ordre des arbres : N(s,a) et Ñ(s,a) s'additionnent,
/// Q(s,a) et Q̃(s,a) sont les moyennes pondérées, et les bornes prouvées par
/// le solveur se combinent (la meilleure borne de chaque côté). Le coup est
/// choisi par la stratégie finale sur les statistiques fusionnées.
pub struct RootParallelAgent<G: Game, A: TreeAlgorithm<G>, P: RolloutPolicy<G>> {
    workers: Vec<Mcts<G, A, P>>,
    rngs: Vec<Pcg32>,
    budget: Box<dyn Budget>,
    label: String,
    played: Vec<G::Move>,
}

impl<G, A, P> RootParallelAgent<G, A, P>
where
    G: Game,
    A: TreeAlgorithm<G>,
    P: RolloutPolicy<G>,
{
    /// Assemble un joueur à partir d'au moins un moteur. Les générateurs des
    /// arbres sont dérivés de `rng` (flux distincts, reproductibles).
    #[must_use]
    pub fn new(
        workers: Vec<Mcts<G, A, P>>,
        budget: Box<dyn Budget>,
        mut rng: Pcg32,
        label: String,
    ) -> Self {
        assert!(!workers.is_empty(), "au moins un arbre");
        let seed = rng.next_u64();
        let rngs = (0..workers.len() as u64)
            .map(|k| Pcg32::new(derive_seed(seed, k), 0x5052 + k))
            .collect();
        RootParallelAgent {
            workers,
            rngs,
            budget,
            label,
            played: Vec::new(),
        }
    }

    /// Nombre d'arbres.
    #[must_use]
    pub fn trees(&self) -> usize {
        self.workers.len()
    }
}

/// Statistiques fusionnées d'un coup de la racine.
#[derive(Clone, Copy)]
struct Merged<M> {
    mv: M,
    n: u64,
    q_sum: f64,
    amaf_n: u64,
    amaf_sum: f64,
    lower: u8,
    upper: u8,
}

impl<G, A, P> Agent<G> for RootParallelAgent<G, A, P>
where
    G: Game + Send + Sync,
    G::Move: Send + Sync,
    A: TreeAlgorithm<G>,
    P: RolloutPolicy<G> + Send,
{
    fn label(&self) -> &str {
        &self.label
    }

    fn observe(&mut self, mv: G::Move) {
        self.played.push(mv);
    }

    fn choose(
        &mut self,
        state: &G,
        cancel: &CancelToken,
    ) -> Result<SearchReport<G::Move>, SearchError> {
        let parts = self.workers.len();
        let mut budgets: Vec<Box<dyn Budget>> = Vec::with_capacity(parts);
        for k in 0..parts {
            match self.budget.share(k, parts) {
                Some(b) => budgets.push(b),
                None => {
                    // Budget non partageable : un seul arbre.
                    let report = self.workers[0].search_after(
                        state,
                        &self.played,
                        &mut *self.budget,
                        &mut self.rngs[0],
                        cancel,
                    );
                    self.played.clear();
                    return report;
                }
            }
        }
        let started = std::time::Instant::now();
        let played = std::mem::take(&mut self.played);
        let results: Vec<Result<SearchReport<G::Move>, SearchError>> =
            std::thread::scope(|scope| {
                let handles: Vec<_> = self
                    .workers
                    .iter_mut()
                    .zip(self.rngs.iter_mut())
                    .zip(budgets.iter_mut())
                    .map(|((worker, rng), budget)| {
                        let played = &played;
                        scope.spawn(move || {
                            worker.search_after(state, played, &mut **budget, rng, cancel)
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| {
                        h.join().unwrap_or_else(|_| {
                            Err(SearchError::InvalidConfig("panique d'un arbre".into()))
                        })
                    })
                    .collect()
            });
        let mut reports = Vec::with_capacity(parts);
        for r in results {
            reports.push(r?);
        }
        Ok(self.merge(&reports, started))
    }
}

impl<G, A, P> RootParallelAgent<G, A, P>
where
    G: Game,
    A: TreeAlgorithm<G>,
    P: RolloutPolicy<G>,
{
    fn merge(
        &self,
        reports: &[SearchReport<G::Move>],
        started: std::time::Instant,
    ) -> SearchReport<G::Move> {
        let first = &self.workers[0];
        let solver = first.config().solver;
        let root0 = *first.tree().node(0);
        let mut merged: Vec<Merged<G::Move>> = first
            .tree()
            .edges_of(0)
            .moves
            .iter()
            .map(|&mv| Merged {
                mv,
                n: 0,
                q_sum: 0.0,
                amaf_n: 0,
                amaf_sum: 0.0,
                lower: 0,
                upper: 2,
            })
            .collect();
        let mut visits = 0u64;
        let mut root_lower = 0u8;
        let mut root_upper = 2u8;
        for worker in &self.workers {
            let tree = worker.tree();
            let root = tree.node(0);
            visits += u64::from(root.visits);
            root_lower = root_lower.max(root.proof.lower());
            root_upper = root_upper.min(root.proof.upper());
            for e in tree.edges_of(0) {
                let Some(m) = merged.iter_mut().find(|m| m.mv == e.mv) else {
                    continue;
                };
                m.n += u64::from(e.n);
                m.q_sum += f64::from(e.q) * f64::from(e.n);
                if let Some(a) = worker.algorithm().amaf(&e) {
                    m.amaf_n += u64::from(a.n);
                    m.amaf_sum += f64::from(a.q) * f64::from(a.n);
                }
                if solver && e.child != NO_NODE {
                    let child = tree.node(e.child);
                    let proof = if child.player == root.player {
                        child.proof
                    } else {
                        child.proof.flipped()
                    };
                    m.lower = m.lower.max(proof.lower());
                    m.upper = m.upper.min(proof.upper());
                }
            }
        }
        let to_u32 = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
        let node = Node {
            visits: to_u32(visits),
            ..root0
        };
        let edges: Vec<Edge<G::Move, A::Extra>> = merged
            .iter()
            .map(|m| {
                let q = if m.n > 0 {
                    (m.q_sum / m.n as f64) as f32
                } else {
                    0.0
                };
                // L'AMAF fusionnée ne sert qu'aux rapports et à la valeur :
                // les autres statistiques propres à l'algorithme valent leur valeur par défaut.
                Edge {
                    mv: m.mv,
                    child: NO_NODE,
                    n: to_u32(m.n),
                    q,
                    inv_sqrt_n: crate::tree::inv_sqrt_visits(to_u32(m.n)),
                    extra: A::Extra::default(),
                }
            })
            .collect();
        let amaf: Vec<AmafStats> = merged
            .iter()
            .map(|m| AmafStats {
                n: to_u32(m.amaf_n),
                q: if m.amaf_n > 0 {
                    (m.amaf_sum / m.amaf_n as f64) as f32
                } else {
                    0.0
                },
            })
            .collect();
        // Valeur de chaque coup : celle de l'algorithme sur les statistiques
        // fusionnées (Q* pour RAVE, d'après la moyenne pondérée de l'AMAF).
        let summaries: Vec<CandidateSummary> = edges
            .iter()
            .zip(&amaf)
            .map(|(e, a)| CandidateSummary {
                visits: e.n,
                value: merged_value(first, &node, e, *a),
            })
            .collect();
        let bounds: Vec<(u8, u8)> = merged.iter().map(|m| (m.lower, m.upper)).collect();
        let root_proof = Proof::new(root_lower, root_upper.max(root_lower));
        let strategy = first.final_move_strategy();
        let best = if solver {
            crate::search::choose_final(strategy, &summaries, &bounds, root_proof)
        } else {
            strategy.choose(&summaries).unwrap_or(0)
        };
        let sims: u64 = reports.iter().map(|r| r.simulations).sum();
        let weighted = |f: &dyn Fn(&SearchReport<G::Move>) -> f64| -> f64 {
            if sims == 0 {
                0.0
            } else {
                reports
                    .iter()
                    .map(|r| f(r) * r.simulations as f64)
                    .sum::<f64>()
                    / sims as f64
            }
        };
        let mut candidates = Vec::new();
        if first.config().detail == ReportDetail::Full {
            candidates = edges
                .iter()
                .zip(&amaf)
                .zip(&summaries)
                .zip(&bounds)
                .map(|(((e, a), s), &(lo, hi))| CandidateStats {
                    mv: e.mv,
                    visits: e.n,
                    q: e.q,
                    amaf_visits: a.n,
                    amaf_q: a.q,
                    value: s.value,
                    proven: (solver && lo == hi).then_some(lo),
                })
                .collect();
            candidates.sort_by(crate::report::candidate_order);
        }
        SearchReport {
            best_move: edges[best].mv,
            simulations: sims,
            nodes: reports.iter().map(|r| r.nodes).sum(),
            max_depth: reports.iter().map(|r| r.max_depth).max().unwrap_or(0),
            mean_depth: weighted(&|r| f64::from(r.mean_depth)) as f32,
            root_value: if sims > 0 {
                weighted(&|r| f64::from(r.root_value)) as f32
            } else {
                0.5
            },
            best_visits: edges[best].n,
            best_value: summaries[best].value,
            node_limit_reached: reports.iter().any(|r| r.node_limit_reached),
            elapsed_us: started.elapsed().as_micros() as u64,
            reused_visits: reports.iter().map(|r| r.reused_visits).sum(),
            proven: (solver && root_lower >= root_upper).then_some(root_lower),
            candidates,
        }
    }
}

/// Valeur d'un coup fusionné selon l'algorithme : l'arête synthétique reçoit
/// l'AMAF fusionnée si l'algorithme en maintient une.
fn merged_value<G, A, P>(
    engine: &Mcts<G, A, P>,
    node: &Node,
    edge: &Edge<G::Move, A::Extra>,
    amaf: AmafStats,
) -> f32
where
    G: Game,
    A: TreeAlgorithm<G>,
    P: RolloutPolicy<G>,
{
    let algorithm = engine.algorithm();
    if algorithm.amaf(edge).is_some() {
        let prior = crate::prior::Prior {
            value: amaf.q,
            confidence: 0,
            amaf_confidence: amaf.n,
        };
        let with_amaf = Edge {
            extra: algorithm.extra_from_prior(&prior),
            ..*edge
        };
        algorithm.value(node, &with_amaf)
    } else {
        algorithm.value(node, edge)
    }
}

/// Joueur aléatoire uniforme (référence et contrôle « budget minimal »).
pub struct RandomAgent<G: Game> {
    rng: Pcg32,
    buf: Vec<G::Move>,
    label: String,
}

impl<G: Game> RandomAgent<G> {
    /// Joueur aléatoire.
    #[must_use]
    pub fn new(rng: Pcg32) -> Self {
        RandomAgent {
            rng,
            buf: Vec::new(),
            label: "random".to_owned(),
        }
    }
}

impl<G> Agent<G> for RandomAgent<G>
where
    G: Game + Send,
    G::Move: Send,
{
    fn label(&self) -> &str {
        &self.label
    }

    fn choose(
        &mut self,
        state: &G,
        cancel: &CancelToken,
    ) -> Result<SearchReport<G::Move>, SearchError> {
        if cancel.is_cancelled() {
            return Err(SearchError::Cancelled);
        }
        let mv = state
            .random_legal_move(&mut self.rng, &mut self.buf)
            .ok_or(SearchError::TerminalPosition)?;
        Ok(SearchReport::without_search(mv))
    }
}
