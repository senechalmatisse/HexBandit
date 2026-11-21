//! Moteur MCTS générique.
//!
//! Une simulation suit les quatre phases de l'article :
//!
//! 1. **sélection** dans l'arbre par la politique de l'algorithme ;
//! 2. **expansion** : un nœud ajouté par simulation (paramétrable par
//!    `expand_threshold`), dans la limite de `max_nodes` ;
//! 3. **simulation** par la politique par défaut ; son premier coup est
//!    attribué au nœud créé, comme dans l'algorithme 2 de l'article ;
//! 4. **rétropropagation** Monte-Carlo (éq. 4-6), puis complément propre à
//!    l'algorithme (AMAF pour RAVE, éq. 16-17).
//!
//! Options désactivées par défaut :
//!
//! * **MCTS-Solver** (`solver`) : bornes prouvées de la valeur de chaque
//!   nœud (défaite, nul, victoire), propagées depuis les positions
//!   terminales (« score bounded MCTS », qui gère aussi les nuls) ; un nœud
//!   résolu n'est plus exploré, les coups prouvés sans intérêt sont écartés de
//!   la sélection et la recherche s'arrête dès que la racine est résolue ;
//! * **réutilisation de l'arbre** (`reuse_tree`) : la recherche suivante
//!   repart du sous-arbre du coup joué au lieu d'un arbre vide (voir [`Mcts::search_after`]).

use std::time::Instant;

use game_core::{
    Game, Outcome,
    rng::Rng,
    rollout::{RolloutPolicy, Trace, TraceMode},
};

use crate::{
    algorithm::{PathStep, TreeAlgorithm},
    budget::Budget,
    cancel::CancelToken,
    final_move::{CandidateSummary, FinalMoveStrategy, MostVisited},
    prior::PriorKnowledge,
    report::{CandidateStats, ReportDetail, SearchError, SearchReport},
    tree::{NO_NODE, Node, Proof, Tree},
};

/// Paramètres du moteur indépendants de l'algorithme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchConfig {
    /// Nombre de passages par une arête avant de créer le nœud enfant
    /// (1 = un nœud ajouté à chaque simulation, comme dans l'article).
    pub expand_threshold: u32,
    /// Nombre maximal de nœuds. Au-delà, la recherche continue sans expansion.
    pub max_nodes: u32,
    /// Niveau de détail du rapport.
    pub detail: ReportDetail,
    /// MCTS-Solver : propagation des positions prouvées.
    pub solver: bool,
    /// Réutilisation du sous-arbre du coup joué d'une recherche à l'autre.
    pub reuse_tree: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            expand_threshold: 1,
            max_nodes: 250_000,
            detail: ReportDetail::Summary,
            solver: false,
            reuse_tree: false,
        }
    }
}

impl SearchConfig {
    /// Vérifie la cohérence des paramètres.
    ///
    /// # Errors
    ///
    /// [`SearchError::InvalidConfig`] si un paramètre est nul.
    pub fn validate(&self) -> Result<(), SearchError> {
        if self.expand_threshold == 0 {
            return Err(SearchError::InvalidConfig(
                "expand_threshold doit être ≥ 1".into(),
            ));
        }
        if self.max_nodes == 0 {
            return Err(SearchError::InvalidConfig("max_nodes doit être ≥ 1".into()));
        }
        Ok(())
    }
}

/// Moteur de recherche, générique (monomorphisé) sur le jeu, l'algorithme et
/// la politique de simulation. Les tampons sont réutilisés d'une
/// recherche à l'autre : aucune allocation en régime établi.
pub struct Mcts<G: Game, A: TreeAlgorithm<G>, P: RolloutPolicy<G>> {
    config: SearchConfig,
    algorithm: A,
    rollout: P,
    final_move: Box<dyn FinalMoveStrategy>,
    prior: Option<Box<dyn PriorKnowledge<G>>>,
    tree: Tree<G::Move, A::Extra>,
    /// Nombre de coups joués à la racine de l'arbre courant (réutilisation).
    tree_ply: Option<u32>,
    scratch: Option<G>,
    path: Vec<PathStep>,
    trace: Trace<G::Move>,
    move_buf: Vec<G::Move>,
}

impl<G, A, P> std::fmt::Debug for Mcts<G, A, P>
where
    G: Game,
    A: TreeAlgorithm<G>,
    P: RolloutPolicy<G>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mcts")
            .field("algorithm", &self.algorithm.name())
            .field("config", &self.config)
            .field("final_move", &self.final_move.name())
            .field("nodes", &self.tree.num_nodes())
            .finish()
    }
}

/// Résultat d'une simulation.
struct SimulationResult {
    outcome: Outcome,
    depth: u32,
    node_limit_hit: bool,
}

/// Bornes (inférieure, supérieure) de la valeur du coup menant à `child`
/// pour le joueur `mover`, en demi-points.
#[inline]
fn move_bounds(mover: game_core::Player, child: &Node) -> (u8, u8) {
    let proof = if child.player == mover {
        child.proof
    } else {
        child.proof.flipped()
    };
    (proof.lower(), proof.upper())
}

impl<G, A, P> Mcts<G, A, P>
where
    G: Game,
    A: TreeAlgorithm<G>,
    P: RolloutPolicy<G>,
{
    /// Moteur avec choix final « plus visité » et sans connaissance a priori.
    /// Préférer [`crate::MctsBuilder`], qui valide la configuration.
    #[must_use]
    pub fn new(config: SearchConfig, algorithm: A, rollout: P) -> Self {
        let mut tree = Tree::new();
        tree.set_keep_inv_sqrt(algorithm.needs_inv_sqrt());
        Mcts {
            config,
            algorithm,
            rollout,
            final_move: Box::new(MostVisited),
            prior: None,
            tree,
            tree_ply: None,
            scratch: None,
            path: Vec::new(),
            trace: Trace::new(TraceMode::Off),
            move_buf: Vec::new(),
        }
    }

    /// Remplace la stratégie de choix final.
    pub fn set_final_move(&mut self, strategy: Box<dyn FinalMoveStrategy>) {
        self.final_move = strategy;
    }

    /// Installe une connaissance a priori.
    pub fn set_prior(&mut self, prior: Option<Box<dyn PriorKnowledge<G>>>) {
        self.prior = prior;
    }

    /// Configuration du moteur.
    #[must_use]
    pub fn config(&self) -> &SearchConfig {
        &self.config
    }

    /// Algorithme (lecture seule).
    #[must_use]
    pub fn algorithm(&self) -> &A {
        &self.algorithm
    }

    /// Arbre de la dernière recherche.
    #[must_use]
    pub fn tree(&self) -> &Tree<G::Move, A::Extra> {
        &self.tree
    }

    /// Stratégie de choix final.
    #[must_use]
    pub fn final_move_strategy(&self) -> &dyn FinalMoveStrategy {
        self.final_move.as_ref()
    }

    /// Lance une recherche depuis `root`, sur un arbre vide, et renvoie le
    /// coup choisi et le rapport.
    ///
    /// # Errors
    ///
    /// * [`SearchError::TerminalPosition`] si `root` est terminale ;
    /// * [`SearchError::Cancelled`] si `cancel` est déclenché ;
    /// * [`SearchError::IllegalMove`] en cas d'incohérence interne.
    pub fn search<R: Rng + ?Sized>(
        &mut self,
        root: &G,
        budget: &mut dyn Budget,
        rng: &mut R,
        cancel: &CancelToken,
    ) -> Result<SearchReport<G::Move>, SearchError> {
        self.tree_ply = None;
        self.search_after(root, &[], budget, rng, cancel)
    }

    /// Recherche depuis `root`, atteinte par les coups `played` depuis la
    /// racine de la recherche précédente. Avec `reuse_tree`, le sous-arbre
    /// correspondant est conservé (ses statistiques s'ajoutent au budget de
    /// cette recherche) ; sinon, ou si l'arbre ne contient pas ce chemin,
    /// l'arbre est vidé comme dans [`Mcts::search`].
    ///
    /// # Errors
    ///
    /// Comme [`Mcts::search`].
    pub fn search_after<R: Rng + ?Sized>(
        &mut self,
        root: &G,
        played: &[G::Move],
        budget: &mut dyn Budget,
        rng: &mut R,
        cancel: &CancelToken,
    ) -> Result<SearchReport<G::Move>, SearchError> {
        let started = Instant::now();
        if root.is_terminal() {
            return Err(SearchError::TerminalPosition);
        }
        self.algorithm.begin_search(root);
        let reused = self.config.reuse_tree && self.reuse_subtree(root, played);
        if !reused {
            self.tree.clear();
            create_node(
                &mut self.tree,
                root,
                rng,
                &mut self.move_buf,
                self.prior.as_deref(),
                &self.algorithm,
            );
            self.algorithm
                .on_node_created(root, 0, self.tree.edges_of(0));
        }
        self.tree_ply = Some(root.ply());
        let reused_visits = if reused { self.tree.node(0).visits } else { 0 };
        if let Some(sims) = budget.expected_simulations() {
            // Environ un nœud toutes les `expand_threshold` simulations, et au
            // plus autant d'arêtes par nœud qu'à la racine : réserver évite les
            // réallocations (copies de tout l'arbre) pendant la recherche.
            // Simple estimation, plafonnée par max_nodes ; sans effet sur les résultats.
            let nodes = (sims / u64::from(self.config.expand_threshold.max(1)))
                .saturating_add(self.tree.num_nodes() as u64)
                .min(u64::from(self.config.max_nodes) + 1) as usize;
            let edges = nodes.saturating_mul(self.tree.edges_of(0).len());
            self.tree.reserve(nodes, edges);
            self.algorithm.reserve_nodes(nodes);
        }
        let root_player = root.current_player();
        budget.start();
        let mut sims: u64 = 0;
        let mut reward_sum = 0.0f64;
        let mut max_depth = 0u32;
        let mut depth_sum = 0u64;
        let mut node_limit_reached = false;
        while budget.allows_next(sims) {
            if self.config.solver && self.tree.node(0).proof.is_solved() {
                // Racine résolue : plus rien à apprendre.
                break;
            }
            if sims % 64 == 0 && cancel.is_cancelled() {
                return Err(SearchError::Cancelled);
            }
            let result = self.simulate(root, rng)?;
            reward_sum += f64::from(result.outcome.reward_for(root_player));
            max_depth = max_depth.max(result.depth);
            depth_sum += u64::from(result.depth);
            node_limit_reached |= result.node_limit_hit;
            sims += 1;
        }
        let mean_depth = if sims > 0 {
            depth_sum as f32 / sims as f32
        } else {
            0.0
        };
        Ok(self.report(
            sims,
            reward_sum,
            (max_depth, mean_depth),
            node_limit_reached,
            reused_visits,
            started,
        ))
    }

    /// Descend de la racine courante le long de `played` et, si le chemin
    /// existe dans l'arbre, ne garde que le sous-arbre atteint.
    fn reuse_subtree(&mut self, root: &G, played: &[G::Move]) -> bool {
        let Some(ply) = self.tree_ply else {
            return false;
        };
        if self.tree.num_nodes() == 0
            || played.is_empty()
            || u64::from(ply) + played.len() as u64 != u64::from(root.ply())
        {
            return false;
        }
        let mut node = 0u32;
        for &mv in played {
            let edges = self.tree.edges_of(node);
            let Some(local) = edges.moves.iter().position(|&m| m == mv) else {
                return false;
            };
            let child = edges.children[local];
            if child == NO_NODE {
                return false;
            }
            node = child;
        }
        let new_root = *self.tree.node(node);
        if new_root.terminal.is_some()
            || new_root.player != root.current_player()
            || new_root.num_edges == 0
        {
            return false;
        }
        let old_ids = self.tree.retain_subtree(node);
        self.algorithm.on_tree_rebuilt(root, &self.tree, &old_ids);
        true
    }

    fn report(
        &self,
        sims: u64,
        reward_sum: f64,
        (max_depth, mean_depth): (u32, f32),
        node_limit_reached: bool,
        reused_visits: u32,
        started: Instant,
    ) -> SearchReport<G::Move> {
        let root = *self.tree.node(0);
        let edges = self.tree.edges_of(0);
        let summaries: Vec<CandidateSummary> = edges
            .iter()
            .map(|e| CandidateSummary {
                visits: e.n,
                value: self.algorithm.value(&root, &e),
            })
            .collect();
        let bounds: Vec<(u8, u8)> = edges
            .children
            .iter()
            .map(|&c| {
                if self.config.solver && c != NO_NODE {
                    move_bounds(root.player, self.tree.node(c))
                } else {
                    (0, 2)
                }
            })
            .collect();
        let best = if self.config.solver {
            choose_final(&*self.final_move, &summaries, &bounds, root.proof)
        } else {
            self.final_move.choose(&summaries).unwrap_or(0)
        };
        let best_edge = edges.get(best);
        let mut candidates = Vec::new();
        if self.config.detail == ReportDetail::Full {
            candidates = edges
                .iter()
                .zip(&summaries)
                .zip(&bounds)
                .map(|((e, s), &(lower, upper))| {
                    let amaf = self.algorithm.amaf(&e).unwrap_or_default();
                    CandidateStats {
                        mv: e.mv,
                        visits: e.n,
                        q: e.q,
                        amaf_visits: amaf.n,
                        amaf_q: amaf.q,
                        value: s.value,
                        proven: (lower == upper).then_some(lower),
                    }
                })
                .collect();
            candidates.sort_by(crate::report::candidate_order);
        }
        SearchReport {
            best_move: best_edge.mv,
            simulations: sims,
            nodes: self.tree.num_nodes() as u32,
            max_depth,
            mean_depth,
            root_value: if sims > 0 {
                (reward_sum / sims as f64) as f32
            } else {
                0.5
            },
            best_visits: best_edge.n,
            best_value: summaries[best].value,
            node_limit_reached,
            elapsed_us: started.elapsed().as_micros() as u64,
            reused_visits,
            proven: (self.config.solver && root.proof.is_solved()).then(|| root.proof.lower()),
            candidates,
        }
    }

    /// Une simulation complète (sélection, expansion, simulation, rétropropagation).
    fn simulate<R: Rng + ?Sized>(
        &mut self,
        root: &G,
        rng: &mut R,
    ) -> Result<SimulationResult, SearchError> {
        let Mcts {
            config,
            algorithm,
            rollout,
            prior,
            tree,
            scratch,
            path,
            trace,
            move_buf,
            ..
        } = self;
        let state = match scratch {
            Some(s) => {
                s.clone_from(root);
                s
            }
            None => scratch.insert(root.clone()),
        };
        path.clear();
        trace.clear();
        trace.set_mode(algorithm.trace_mode());
        let mut node_id = 0u32;
        let mut terminal_node = None;
        let mut new_terminal = false;
        let mut node_limit_hit = false;

        let outcome = loop {
            let node = *tree.node(node_id);
            if let Some(o) = node.terminal {
                terminal_node = Some(node_id);
                break o;
            }
            if config.solver && node.proof.is_solved() {
                // Position résolue : sa valeur exacte tient lieu de simulation.
                terminal_node = Some(node_id);
                break match node.proof.lower() {
                    2 => Outcome::Win(node.player),
                    0 => Outcome::Win(node.player.opponent()),
                    _ => Outcome::Draw,
                };
            }
            // 1. Sélection (les coups prouvés sans intérêt sont écartés).
            let local = if config.solver && node.proof.informed() {
                let lower = node.proof.lower();
                let first = node.first_edge;
                let tree_ref: &Tree<G::Move, A::Extra> = tree;
                let exclude = move |i: usize| {
                    let child = tree_ref.edge_child(first + i as u32);
                    child != NO_NODE && move_bounds(node.player, tree_ref.node(child)).1 <= lower
                };
                algorithm.select(root, tree_ref, node_id, Some(&exclude))
            } else {
                algorithm.select(root, tree, node_id, None)
            };
            let edge_id = node.first_edge + local as u32;
            let (mv, child, visits) = {
                let e = tree.edge(edge_id);
                (e.mv, e.child, e.n)
            };
            state.play(mv)?;
            path.push(PathStep {
                node: node_id,
                edge: edge_id,
            });
            if child != NO_NODE {
                node_id = child;
                continue;
            }
            // 2. Expansion (au plus un nœud par simulation).
            let wants_expansion = visits + 1 >= config.expand_threshold;
            if wants_expansion && tree.num_nodes() as u32 >= config.max_nodes {
                node_limit_hit = true;
            }
            if wants_expansion && !node_limit_hit {
                let child = create_node(tree, state, rng, move_buf, prior.as_deref(), &*algorithm);
                tree.set_child(edge_id, child);
                if let Some(o) = tree.node(child).terminal {
                    algorithm.on_node_created(root, child, tree.edges_of(child));
                    terminal_node = Some(child);
                    new_terminal = true;
                    break o;
                }
                // 3. Simulation ; le premier coup est attribué au nouveau nœud (algorithme 2).
                //    Son arête est placée en tête : l'ordre des
                //    arêtes reste uniformément aléatoire (premier coup tiré au
                //    hasard) et les arêtes essayées forment un préfixe.
                let o = rollout.rollout(state, rng, trace);
                if let Some(&(first, _)) = trace.entries().first() {
                    let first_edge = tree.node(child).first_edge;
                    if let Some(pos) = tree.edges_of(child).moves.iter().position(|&m| m == first) {
                        tree.swap_edges(first_edge, first_edge + pos as u32);
                        path.push(PathStep {
                            node: child,
                            edge: first_edge,
                        });
                    }
                }
                // L'ordre des arêtes est définitif : l'algorithme peut l'indexer.
                algorithm.on_node_created(root, child, tree.edges_of(child));
                break o;
            }
            // 3'. Simulation sans expansion.
            break rollout.rollout(state, rng, trace);
        };

        // 4. Rétropropagation Monte-Carlo (éq. 4-6), du point de vue du joueur qui joue chaque coup.
        for step in path.iter() {
            let node = tree.node_mut(step.node);
            node.visits += 1;
            let z = outcome.reward_for(node.player);
            tree.record_visit(step.edge, z);
        }
        if let Some(t) = terminal_node {
            tree.node_mut(t).visits += 1;
        }
        algorithm.backup_extra(root, tree, path, trace.entries(), outcome);
        if config.solver && new_terminal {
            propagate_proofs(tree, path);
        }
        Ok(SimulationResult {
            outcome,
            depth: path.len() as u32,
            node_limit_hit,
        })
    }
}

/// Remonte le chemin en recalculant les bornes de chaque nœud à partir de ses enfants
/// (une arête sans enfant vaut [défaite, victoire]) ; s'arrête dès qu'un nœud ne change plus.
fn propagate_proofs<M: Copy, X: Copy + Default>(tree: &mut Tree<M, X>, path: &[PathStep]) {
    for step in path.iter().rev() {
        let node = *tree.node(step.node);
        let mut lower = 0u8;
        let mut upper = 0u8;
        let mut informed = false;
        for &child in tree.edges_of(step.node).children {
            let (lo, hi) = if child == NO_NODE {
                (0, 2)
            } else {
                let c = tree.node(child);
                informed |= c.proof.lower() != 0 || c.proof.upper() != 2;
                move_bounds(node.player, c)
            };
            lower = lower.max(lo);
            upper = upper.max(hi);
        }
        let proof = Proof::new(lower, upper).with_informed(informed);
        if proof == node.proof {
            break;
        }
        tree.node_mut(step.node).proof = proof;
    }
}

/// Choix final avec le solveur : un coup prouvé gagnant s'il en existe ;
/// sinon la stratégie habituelle parmi les coups qui ne sont pas des
/// défaites prouvées (tous si la position est perdue).
pub(crate) fn choose_final(
    strategy: &dyn FinalMoveStrategy,
    summaries: &[CandidateSummary],
    bounds: &[(u8, u8)],
    root: Proof,
) -> usize {
    let pick = |keep: &dyn Fn(usize) -> bool| -> Option<usize> {
        let indices: Vec<usize> = (0..summaries.len()).filter(|&i| keep(i)).collect();
        let subset: Vec<CandidateSummary> = indices.iter().map(|&i| summaries[i]).collect();
        strategy.choose(&subset).map(|j| indices[j])
    };
    if root.lower() == 2 {
        if let Some(i) = pick(&|i| bounds[i].0 == 2) {
            return i;
        }
    }
    pick(&|i| bounds[i].1 > 0)
        .or_else(|| strategy.choose(summaries))
        .unwrap_or(0)
}

/// Crée un nœud pour `state` et l'initialise avec la connaissance a priori.
/// L'appelant prévient l'algorithme une fois l'ordre des arêtes fixé.
fn create_node<G, A, R>(
    tree: &mut Tree<G::Move, A::Extra>,
    state: &G,
    rng: &mut R,
    buf: &mut Vec<G::Move>,
    prior: Option<&dyn PriorKnowledge<G>>,
    algorithm: &A,
) -> u32
where
    G: Game,
    A: TreeAlgorithm<G>,
    R: Rng + ?Sized,
{
    let id = tree.push_node(state, rng, buf);
    if let Some(prior) = prior {
        let mut total = 0u32;
        let first = tree.node(id).first_edge;
        for i in 0..tree.node(id).num_edges {
            let edge = first + i;
            if let Some(p) = prior.prior(state, tree.edge_move(edge)) {
                tree.set_statistics(edge, p.confidence, p.value, algorithm.extra_from_prior(&p));
                total += p.confidence;
            }
        }
        tree.node_mut(id).visits = total;
    }
    id
}
