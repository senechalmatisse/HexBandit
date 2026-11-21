//! Politiques d'arbre : UCT et MC-RAVE (avec sa généralisation GRAVE).
//!
//! Le moteur ([`crate::Mcts`]) fixe le squelette d'une simulation (Template
//! Method) ; l'algorithme fournit les étapes variables : sélection d'une
//! arête, statistiques supplémentaires et rétropropagation complémentaire.

use game_core::{AmafGame, Game, Outcome, Player, math, rollout::TraceMode};

use crate::{
    beta::{BetaSchedule, ScheduleKind},
    prior::Prior,
    tree::{AmafStats, Edge, Edges, Node, Tree},
};

/// Étape du chemin d'une simulation dans l'arbre : nœud et arête suivie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathStep {
    /// Indice du nœud.
    pub node: u32,
    /// Indice global de l'arête choisie.
    pub edge: u32,
}

/// Arêtes à écarter de la sélection (MCTS-Solver : coups prouvés sans intérêt).
/// `None` dans le cas courant, où toutes les arêtes sont candidates.
pub type Exclusion<'a> = Option<&'a dyn Fn(usize) -> bool>;

/// Politique d'arbre d'un algorithme MCTS.
pub trait TreeAlgorithm<G: Game>: Send {
    /// Statistiques supplémentaires portées par chaque arête.
    type Extra: Copy + Default + Send + Sync;

    /// Nom court (`"uct"`, `"rave"`).
    fn name(&self) -> &'static str;

    /// Niveau de trace demandé à la politique de simulation.
    fn trace_mode(&self) -> TraceMode;

    /// Vrai si la sélection lit 1/√N(s,a) : le moteur tient alors cette
    /// colonne à jour au lieu de la recalculer à la demande.
    fn needs_inv_sqrt(&self) -> bool {
        true
    }

    /// Début d'une recherche depuis `root` (l'arbre vient d'être vidé ou
    /// réduit au sous-arbre de `root`).
    fn begin_search(&mut self, _root: &G) {}

    /// La recherche créera probablement jusqu'à `nodes` nœuds : occasion de
    /// réserver les structures indexées par nœud. Sans effet sur les résultats.
    fn reserve_nodes(&mut self, _nodes: usize) {}

    /// Un nœud vient d'être créé avec les arêtes `edges` (ordre définitif).
    fn on_node_created(
        &mut self,
        _root: &G,
        _node_id: u32,
        _edges: Edges<'_, G::Move, Self::Extra>,
    ) {
    }

    /// L'arbre vient d'être renuméroté (réutilisation du sous-arbre) : les
    /// structures indexées par nœud sont à reconstruire ; `old_ids[i]` est
    /// l'ancien numéro du nœud `i`. Par défaut,
    /// [`TreeAlgorithm::on_node_created`] est rappelée pour chaque nœud.
    fn on_tree_rebuilt(&mut self, root: &G, tree: &Tree<G::Move, Self::Extra>, old_ids: &[u32]) {
        let _ = old_ids;
        for id in 0..tree.num_nodes() as u32 {
            self.on_node_created(root, id, tree.edges_of(id));
        }
    }

    /// Indice local de l'arête à suivre depuis le nœud `node_id` (au moins
    /// une arête). Les arêtes pour lesquelles `exclude` est vrai ne sont pas
    /// candidates (il en reste au moins une).
    fn select(
        &mut self,
        root: &G,
        tree: &Tree<G::Move, Self::Extra>,
        node_id: u32,
        exclude: Exclusion<'_>,
    ) -> usize;

    /// Rétropropagation complémentaire, appelée après la mise à jour Monte-Carlo commune (éq. 4-6).
    /// `rollout` contient les coups joués hors de l'arbre, dans l'ordre.
    fn backup_extra(
        &mut self,
        root: &G,
        tree: &mut Tree<G::Move, Self::Extra>,
        path: &[PathStep],
        rollout: &[(G::Move, Player)],
        outcome: Outcome,
    );

    /// Valeur de l'action utilisée pour le choix final « meilleure valeur » et
    /// les rapports (Q pour UCT, Q* pour RAVE).
    fn value(&self, node: &Node, edge: &Edge<G::Move, Self::Extra>) -> f32;

    /// Statistiques AMAF de l'arête, si l'algorithme en maintient.
    fn amaf(&self, edge: &Edge<G::Move, Self::Extra>) -> Option<AmafStats>;

    /// Statistiques supplémentaires initiales issues d'une connaissance a priori (éq. 48-51).
    fn extra_from_prior(&self, prior: &Prior) -> Self::Extra;
}

/// Argmax générique (premier indice en cas d'égalité) avec exclusions : chemin
/// lent, emprunté seulement par le solveur sur les nœuds dont un enfant est résolu.
fn argmax_excluding(
    len: usize,
    exclude: &dyn Fn(usize) -> bool,
    score: impl Fn(usize) -> f32,
) -> usize {
    let mut best = usize::MAX;
    let mut best_score = f32::NEG_INFINITY;
    for i in 0..len {
        if exclude(i) {
            continue;
        }
        let s = score(i);
        if best == usize::MAX || s > best_score {
            best_score = s;
            best = i;
        }
    }
    if best == usize::MAX { 0 } else { best }
}

/// UCT : sélection UCB1, Q⊕ = Q + c·√(ln N(s) / N(s,a)) (éq. 7-8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Uct {
    /// Constante d'exploration c (défaut √2).
    pub exploration: f32,
}

impl Default for Uct {
    fn default() -> Self {
        Uct {
            exploration: std::f32::consts::SQRT_2,
        }
    }
}

impl Uct {
    /// c·√(ln N(s)), avec un logarithme identique sur toutes les plateformes.
    #[inline]
    fn c_ln(&self, visits: u32) -> f32 {
        self.exploration * math::ln(visits.max(1) as f32).sqrt()
    }
}

impl<G: Game> TreeAlgorithm<G> for Uct
where
    G::Move: Send + Sync,
{
    type Extra = ();

    fn name(&self) -> &'static str {
        "uct"
    }

    fn trace_mode(&self) -> TraceMode {
        TraceMode::FirstMove
    }

    #[inline]
    fn select(
        &mut self,
        _root: &G,
        tree: &Tree<G::Move, ()>,
        node_id: u32,
        exclude: Exclusion<'_>,
    ) -> usize {
        let node = tree.node(node_id);
        let edges = tree.edges_of(node_id);
        let visits = edges.visits;
        if let Some(exclude) = exclude {
            let c_ln = self.c_ln(node.visits);
            if let Some(i) = (0..visits.len()).find(|&i| visits[i] == 0 && !exclude(i)) {
                return i;
            }
            return argmax_excluding(visits.len(), exclude, |i| {
                edges.values[i] + c_ln * edges.inv_sqrt_n(i)
            });
        }
        // Toute action non essayée a un bonus infini. Les arêtes étant
        // mélangées à la création et l'arête du premier coup simulé placée en
        // tête, les actions déjà essayées forment un préfixe de longueur
        // N(s) tant que N(s) < nombre d'arêtes : accès direct en O(1).
        let v = node.visits as usize;
        if v < visits.len() && visits[v] == 0 {
            return v;
        }
        if let Some(i) = visits.iter().position(|&n| n == 0) {
            return i;
        }
        let c_ln = self.c_ln(node.visits);
        let mut best = 0;
        let mut best_score = f32::NEG_INFINITY;
        for (i, (&q, &inv)) in edges.values.iter().zip(edges.inv_sqrt).enumerate() {
            let score = q + c_ln * inv;
            if score > best_score {
                best_score = score;
                best = i;
            }
        }
        best
    }

    #[inline]
    fn backup_extra(
        &mut self,
        _root: &G,
        _tree: &mut Tree<G::Move, ()>,
        _path: &[PathStep],
        _rollout: &[(G::Move, Player)],
        _outcome: Outcome,
    ) {
    }

    fn value(&self, _node: &Node, edge: &Edge<G::Move, ()>) -> f32 {
        edge.q
    }

    fn amaf(&self, _edge: &Edge<G::Move, ()>) -> Option<AmafStats> {
        None
    }

    fn extra_from_prior(&self, _prior: &Prior) {}
}

/// MC-RAVE, avec bonus d'exploration optionnel (UCT-RAVE, éq. 18) et sa généralisation GRAVE (Cazenave, 2015).
///
/// * Avec `exploration = 0` (défaut).
/// * GRAVE : un nœud visité moins de `grave_ref` fois utilise les
///   statistiques AMAF de son plus proche ancêtre (sur le chemin de la
///   simulation, la racine à défaut) visité au moins `grave_ref` fois, plus
///   fiables, pour les coups du joueur qui a le trait **dans le nœud**. Les
///   arêtes d'un nœud ne portent que l'AMAF des coups de son joueur au trait;
///   si la référence est un nœud de l'adversaire, ces
///   statistiques viennent d'une table par nœud qui retient l'AMAF des coups
///   adverses, tenue à jour seulement quand GRAVE est actif. Avec `grave_ref = 0`
///   (défaut), chaque nœud utilise les siennes : c'est exactement MC-RAVE, sans la table.
#[derive(Clone, Debug)]
pub struct Rave<B> {
    /// Constante d'exploration c ≥ 0.
    pub exploration: f32,
    /// Planning de β.
    pub schedule: B,
    /// Seuil GRAVE (0 = MC-RAVE).
    pub grave_ref: u32,
    /// Marques « coup déjà vu » par joueur, pour le parcours à rebours.
    seen: [Vec<u32>; 2],
    /// Coups distincts vus par joueur, dans l'ordre du parcours à rebours.
    eligible: [Vec<u32>; 2],
    stamp: u32,
    /// Taille de l'espace des indices de coups.
    bound: usize,
    /// Pour chaque nœud, indice de coup -> indice local de l'arête (ou NONE).
    edge_of_move: Vec<u16>,
    /// GRAVE : pour chaque nœud, indice de coup -> AMAF des coups du joueur
    /// qui n'y a pas le trait (vide si GRAVE est inactif).
    opponent_amaf: Vec<AmafStats>,
    /// Nœud de référence GRAVE de la descente en cours.
    reference: u32,
}

/// Absence d'arête pour un indice de coup.
const NO_EDGE: u16 = u16::MAX;

/// Taille des blocs d'arêtes de la sélection vectorisée de MC-RAVE (tampon
/// des scores sur la pile).
const BLOCK: usize = 32;

impl<B: BetaSchedule> Rave<B> {
    /// MC-RAVE avec le planning et la constante d'exploration indiqués.
    #[must_use]
    pub fn new(schedule: B, exploration: f32) -> Self {
        Rave {
            exploration,
            schedule,
            grave_ref: 0,
            seen: [Vec::new(), Vec::new()],
            eligible: [Vec::new(), Vec::new()],
            stamp: 0,
            bound: 0,
            edge_of_move: Vec::new(),
            opponent_amaf: Vec::new(),
            reference: 0,
        }
    }

    /// GRAVE avec le seuil `grave_ref` (0 = MC-RAVE).
    #[must_use]
    pub fn with_grave(mut self, grave_ref: u32) -> Self {
        self.grave_ref = grave_ref;
        self
    }

    /// Valeur combinée Q* (éq. 12), avec β = 1 si N(s,a) = 0 et β = 0 si
    /// Ñ(s,a) = 0. Vaut +∞ si aucune des deux valeurs n'existe.
    #[inline]
    fn combined(&self, parent_visits: u32, n: u32, q: f32, amaf: AmafStats) -> f32 {
        match (n, amaf.n) {
            (0, 0) => f32::INFINITY,
            (0, _) => amaf.q,
            (_, 0) => q,
            _ => q + self.schedule.beta(parent_visits, n, amaf.n) * (amaf.q - q),
        }
    }

    /// Argmax de Q* + `c_ln`·N(s,a)^(-1/2) sur les arêtes, la première en cas
    /// d'égalité, sans branchement dépendant des données dans le calcul des
    /// scores : les trois cas de Q* (N = 0, Ñ = 0, les deux non nuls)
    /// sont calculés puis choisis par masques, ce qui supprime l'essentiel
    /// des erreurs de prédiction du test « arête visitée ou non »
    /// (imprévisible, les arêtes étant mélangées), et le compilateur vectorise
    /// le calcul, fait par blocs dans un tampon sur la pile. Les comptes sont
    /// convertis en réels (exacts pour 0) pour que les conditions soient des
    /// comparaisons de réels : une comparaison d'entiers redevient un
    /// branchement, le choix portant sur le résultat d'une division.
    /// `beta(n, ñ)` est évalué pour toutes les arêtes mais n'est retenu que si
    /// N > 0 et Ñ > 0 : les valeurs sont identiques au bit près à celles de [`Rave::combined`].
    #[inline(always)]
    fn argmax(
        visits: &[u32],
        values: &[f32],
        amaf: &[AmafStats],
        inv_sqrt: &[f32],
        c_ln: f32,
        beta: impl Fn(f32, f32) -> f32,
    ) -> usize {
        let len = visits.len();
        let mut scores = [0.0f32; BLOCK];
        let mut best = 0;
        let mut best_score = f32::NEG_INFINITY;
        let mut start = 0;
        while start < len {
            let end = (start + BLOCK).min(len);
            let width = end - start;
            let (n_blk, q_blk, a_blk) =
                (&visits[start..end], &values[start..end], &amaf[start..end]);
            for j in 0..width {
                let (q, a) = (q_blk[j], a_blk[j]);
                let (n, na) = (n_blk[j] as f32, a.n as f32);
                let mixed = q + beta(n, na) * (a.q - q);
                let unvisited = if na == 0.0 { f32::INFINITY } else { a.q };
                let visited = if na == 0.0 { q } else { mixed };
                scores[j] = if n == 0.0 { unvisited } else { visited };
            }
            if c_ln != 0.0 {
                for (s, &inv) in scores[..width].iter_mut().zip(&inv_sqrt[start..end]) {
                    *s += c_ln * inv;
                }
            }
            for (j, &s) in scores[..width].iter().enumerate() {
                if s > best_score {
                    best_score = s;
                    best = start + j;
                }
            }
            start = end;
        }
        best
    }

    /// Statistiques AMAF de l'arête `i` du nœud courant vues depuis le nœud
    /// de référence GRAVE (coup absent de la référence : aucune statistique).
    #[inline]
    fn reference_amaf<M: Copy>(
        &self,
        reference: Edges<'_, M, AmafStats>,
        map: &[u16],
        index: usize,
    ) -> AmafStats {
        match map[index] {
            NO_EDGE => AmafStats::default(),
            local => reference.extras[usize::from(local)],
        }
    }
}

impl<G, B> TreeAlgorithm<G> for Rave<B>
where
    G: AmafGame,
    G::Move: Send + Sync,
    B: BetaSchedule,
{
    type Extra = AmafStats;

    fn name(&self) -> &'static str {
        if self.grave_ref > 0 { "grave" } else { "rave" }
    }

    fn trace_mode(&self) -> TraceMode {
        TraceMode::Full
    }

    fn needs_inv_sqrt(&self) -> bool {
        self.exploration > 0.0
    }

    fn begin_search(&mut self, root: &G) {
        self.bound = root.move_index_bound();
        self.edge_of_move.clear();
        // La table AMAF adverse de GRAVE est gardée jusqu'à la création de la
        // racine : si l'arbre est réutilisé, `on_tree_rebuilt` la renumérote.
        self.reference = 0;
        for table in &mut self.seen {
            if table.len() < self.bound {
                table.resize(self.bound, 0);
            }
        }
    }

    fn reserve_nodes(&mut self, nodes: usize) {
        let total = nodes.saturating_mul(self.bound);
        let _ = self
            .edge_of_move
            .try_reserve(total.saturating_sub(self.edge_of_move.len()));
        if self.grave_ref > 0 {
            let _ = self
                .opponent_amaf
                .try_reserve(total.saturating_sub(self.opponent_amaf.len()));
        }
    }

    fn on_node_created(&mut self, root: &G, node_id: u32, edges: Edges<'_, G::Move, AmafStats>) {
        let base = node_id as usize * self.bound;
        debug_assert_eq!(self.edge_of_move.len(), base);
        self.edge_of_move.resize(base + self.bound, NO_EDGE);
        for (i, &mv) in edges.moves.iter().enumerate() {
            self.edge_of_move[base + root.move_index(mv)] = i as u16;
        }
        if self.grave_ref > 0 {
            if node_id == 0 {
                self.opponent_amaf.clear();
            }
            debug_assert_eq!(self.opponent_amaf.len(), base);
            self.opponent_amaf
                .resize(base + self.bound, AmafStats::default());
        }
    }

    fn on_tree_rebuilt(&mut self, root: &G, tree: &Tree<G::Move, AmafStats>, old_ids: &[u32]) {
        let old = std::mem::take(&mut self.opponent_amaf);
        self.edge_of_move.clear();
        for id in 0..tree.num_nodes() as u32 {
            self.on_node_created(root, id, tree.edges_of(id));
        }
        if self.grave_ref > 0 {
            let bound = self.bound;
            for (new, &old_id) in old_ids.iter().enumerate() {
                let from = old_id as usize * bound;
                if from + bound <= old.len() {
                    self.opponent_amaf[new * bound..(new + 1) * bound]
                        .copy_from_slice(&old[from..from + bound]);
                }
            }
        }
    }

    /// Sélection gloutonne sur Q* (+ bonus c·√(ln N(s)/max(N(s,a), 1)) si c > 0).
    /// Les actions sans aucune statistique ont Q* = +∞ et passent en premier.
    /// Le planning de β est résolu une fois par nœud ([`BetaSchedule::kind`]) :
    /// la boucle sur les arêtes ne contient ni appel dynamique ni `match`.
    #[inline]
    fn select(
        &mut self,
        root: &G,
        tree: &Tree<G::Move, AmafStats>,
        node_id: u32,
        exclude: Exclusion<'_>,
    ) -> usize {
        let node = tree.node(node_id);
        let parent = node.visits;
        let edges = tree.edges_of(node_id);
        // GRAVE : chaque descente part de la racine, référence par défaut ; un
        // nœud assez visité devient la référence de ses descendants.
        if node_id == 0 || parent >= self.grave_ref {
            self.reference = node_id;
        }
        let reference_id = self.reference;
        let c_ln = if self.exploration > 0.0 {
            self.exploration * math::ln(parent.max(1) as f32).sqrt()
        } else {
            0.0
        };
        if reference_id != node_id || exclude.is_some() {
            // Chemin général : statistiques AMAF de la référence (GRAVE) et/ou
            // exclusions (solveur). Même formule que le chemin rapide.
            let reference = tree.edges_of(reference_id);
            let base = reference_id as usize * self.bound;
            let map = &self.edge_of_move[base..base + self.bound];
            let own = reference_id == node_id;
            // Référence de l'adversaire : AMAF des coups adverses de la référence,
            // c'est-à-dire des coups du joueur au trait ici.
            let opponent = if own || tree.node(reference_id).player == node.player {
                None
            } else {
                Some(&self.opponent_amaf[base..base + self.bound])
            };
            let score = |i: usize| {
                let index = root.move_index(edges.moves[i]);
                let amaf = match (own, opponent) {
                    (true, _) => edges.extras[i],
                    (false, None) => self.reference_amaf(reference, map, index),
                    (false, Some(table)) => table[index],
                };
                let mut s = self.combined(parent, edges.visits[i], edges.values[i], amaf);
                if c_ln != 0.0 {
                    s += c_ln * edges.inv_sqrt_n(i);
                }
                s
            };
            return argmax_excluding(edges.len(), exclude.unwrap_or(&|_| false), score);
        }
        let (visits, values, amaf, inv) =
            (edges.visits, edges.values, edges.extras, edges.inv_sqrt);
        match self.schedule.kind(parent) {
            ScheduleKind::Constant(beta) => {
                Self::argmax(visits, values, amaf, inv, c_ln, |_, _| beta)
            }
            ScheduleKind::MinimumMse { bias } => {
                Self::argmax(visits, values, amaf, inv, c_ln, |n, na| {
                    // Même expression, donc même arrondi, que `MinimumMse::beta`.
                    na / (n + na + 4.0 * n * na * bias * bias)
                })
            }
            ScheduleKind::General => argmax_excluding(edges.len(), &|_| false, |i| {
                let mut s = self.combined(parent, visits[i], values[i], amaf[i]);
                if c_ln != 0.0 {
                    s += c_ln * edges.inv_sqrt_n(i);
                }
                s
            }),
        }
    }

    /// Mise à jour AMAF : pour chaque nœud s_t du
    /// chemin, chaque coup a_u (u ≥ t) joué par le joueur au trait en s_t,
    /// dans l'arbre comme dans la simulation, met à jour Ñ(s_t, a_u) et
    /// Q̃(s_t, a_u), une seule fois par case. Le parcours se fait à rebours :
    /// en arrivant à t, la liste `eligible` du joueur au trait contient
    /// exactement ses coups distincts d'indice u ≥ t ; la table
    /// `edge_of_move` donne l'arête correspondante sans parcourir le nœud.
    fn backup_extra(
        &mut self,
        root: &G,
        tree: &mut Tree<G::Move, AmafStats>,
        path: &[PathStep],
        rollout: &[(G::Move, Player)],
        outcome: Outcome,
    ) {
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            for table in &mut self.seen {
                table.iter_mut().for_each(|v| *v = 0);
            }
            self.stamp = 1;
        }
        let stamp = self.stamp;
        for list in &mut self.eligible {
            list.clear();
        }
        for &(mv, player) in rollout.iter().rev() {
            let idx = root.move_index(mv);
            let p = player.index();
            if self.seen[p][idx] != stamp {
                self.seen[p][idx] = stamp;
                self.eligible[p].push(idx as u32);
            }
        }
        for step in path.iter().rev() {
            let player = tree.node(step.node).player;
            let p = player.index();
            let idx = root.move_index(tree.edge_move(step.edge));
            if self.seen[p][idx] != stamp {
                self.seen[p][idx] = stamp;
                self.eligible[p].push(idx as u32);
            }
            let z = outcome.reward_for(player);
            let base = step.node as usize * self.bound;
            let map = &self.edge_of_move[base..base + self.bound];
            let extras = tree.extras_of_mut(step.node);
            for &idx in &self.eligible[p] {
                let local = map[idx as usize];
                if local == NO_EDGE {
                    // Coup illégal en s_t : pas de mise à jour.
                    continue;
                }
                let amaf = &mut extras[usize::from(local)];
                amaf.n += 1;
                amaf.q += (z - amaf.q) / amaf.n as f32;
            }
            if self.grave_ref > 0 {
                // GRAVE : AMAF des coups de l'adversaire joués après s_t (ils
                // étaient légaux pour lui en s_t : cases encore vides).
                let q = player.opponent();
                let zq = outcome.reward_for(q);
                let table = &mut self.opponent_amaf[base..base + self.bound];
                for &idx in &self.eligible[q.index()] {
                    let amaf = &mut table[idx as usize];
                    amaf.n += 1;
                    amaf.q += (zq - amaf.q) / amaf.n as f32;
                }
            }
        }
    }

    fn value(&self, node: &Node, edge: &Edge<G::Move, AmafStats>) -> f32 {
        self.combined(node.visits, edge.n, edge.q, edge.extra)
    }

    fn amaf(&self, edge: &Edge<G::Move, AmafStats>) -> Option<AmafStats> {
        Some(edge.extra)
    }

    fn extra_from_prior(&self, prior: &Prior) -> AmafStats {
        AmafStats {
            n: prior.amaf_confidence,
            q: prior.value,
        }
    }
}

#[cfg(test)]
mod tests {
    use game_core::rng::Pcg32;
    use game_hex::{BLUE, HexRollout, HexState, RED};

    use super::*;
    use crate::{
        CancelToken, MctsBuilder,
        beta::{AnySchedule, MinimumMse},
        budget::SimulationBudget,
        tree::NO_NODE,
    };

    /// Enfant le plus visité du nœud `id`.
    fn most_visited<M: Copy, X: Copy + Default>(tree: &Tree<M, X>, id: u32) -> u32 {
        let edges = tree.edges_of(id);
        let best = (0..edges.len()).max_by_key(|&i| edges.visits[i]).unwrap();
        assert_ne!(edges.children[best], NO_NODE);
        edges.children[best]
    }

    /// Le chemin général (GRAVE, exclusions du solveur) calcule le même score
    /// que le chemin rapide : sans exclusion effective, même arête choisie.
    #[test]
    fn general_and_fast_selection_paths_agree() {
        let root = HexState::new(6).unwrap();
        for (schedule, exploration) in [
            (AnySchedule::MinimumMse(MinimumMse { bias: 0.02 }), 0.0),
            (AnySchedule::MinimumMse(MinimumMse { bias: 0.02 }), 0.3),
            (
                AnySchedule::HandSelected(crate::beta::HandSelected { k: 1000.0 }),
                0.0,
            ),
        ] {
            let mut engine = MctsBuilder::new(Rave::new(schedule, exploration), HexRollout::fill())
                .build()
                .unwrap();
            let mut rng = Pcg32::new(5, 9);
            engine
                .search(
                    &root,
                    &mut SimulationBudget { simulations: 3000 },
                    &mut rng,
                    &CancelToken::new(),
                )
                .unwrap();
            let tree = engine.tree();
            let mut rave = engine.algorithm().clone();
            let nothing = |_: usize| false;
            let mut checked = 0;
            for id in 0..tree.num_nodes() as u32 {
                if tree.node(id).visits < 2 || tree.edges_of(id).is_empty() {
                    continue;
                }
                let fast = rave.select(&root, tree, id, None);
                let general = rave.select(&root, tree, id, Some(&nothing));
                assert_eq!(fast, general, "nœud {id}");
                checked += 1;
            }
            assert!(checked > 100, "{checked}");
        }
    }

    /// GRAVE : la référence est le nœud le plus profond du chemin visité au
    /// moins `grave_ref` fois (la racine à défaut), quel que soit le joueur qui y a le trait.
    #[test]
    fn grave_reference_is_the_deepest_visited_ancestor() {
        let root = HexState::new(5).unwrap();
        let schedule = AnySchedule::MinimumMse(MinimumMse { bias: 0.02 });
        let mut engine = MctsBuilder::new(
            Rave::new(schedule, 0.0).with_grave(u32::MAX),
            HexRollout::fill(),
        )
        .build()
        .unwrap();
        let mut rng = Pcg32::new(11, 11);
        engine
            .search(
                &root,
                &mut SimulationBudget { simulations: 400 },
                &mut rng,
                &CancelToken::new(),
            )
            .unwrap();
        let tree = engine.tree();
        let c1 = most_visited(tree, 0);
        let c2 = most_visited(tree, c1);
        assert_eq!(tree.node(c1).player, RED);
        assert_eq!(tree.node(c2).player, BLUE);

        // Seuil inatteignable : la racine sert de référence à tout le chemin.
        let mut grave = engine.algorithm().clone();
        for id in [0, c1, c2] {
            grave.select(&root, tree, id, None);
            assert_eq!(grave.reference, 0);
        }
        // Seuil atteint par c1 et non par c2 : c1 devient la référence.
        grave.grave_ref = tree.node(c1).visits;
        assert!(tree.node(c2).visits < grave.grave_ref);
        for (id, expected) in [(0, 0), (c1, c1), (c2, c1)] {
            grave.select(&root, tree, id, None);
            assert_eq!(grave.reference, expected);
        }
    }

    /// Vérifie la table AMAF des coups adverses (GRAVE) : en un nœud, les
    /// coups de l'adversaire joués après lui sont ceux que ses enfants
    /// comptent dans leur propre AMAF. Seule exception : la simulation qui a
    /// créé le nœud, qui n'est passée par aucun de ses enfants (au plus une
    /// partie de plus par coup ; aucune pour une racine créée avant la
    /// recherche). Renvoie le nombre de nœuds vérifiés.
    fn check_opponent_amaf<B: BetaSchedule>(
        tree: &Tree<game_hex::Cell, AmafStats>,
        grave: &Rave<B>,
        fresh_root: bool,
    ) -> usize {
        let bound = grave.bound;
        let mut checked = 0;
        for id in 0..tree.num_nodes() as u32 {
            let edges = tree.edges_of(id);
            if tree.node(id).visits < 20 || edges.is_empty() {
                continue;
            }
            let table = &grave.opponent_amaf[id as usize * bound..(id as usize + 1) * bound];
            let mut n = vec![0u64; bound];
            let mut sum = vec![0f64; bound];
            for &child in edges.children {
                if child == NO_NODE {
                    continue;
                }
                let base = child as usize * bound;
                let map = &grave.edge_of_move[base..base + bound];
                let child_edges = tree.edges_of(child);
                for index in 0..bound {
                    if map[index] != NO_EDGE {
                        let a = child_edges.extras[usize::from(map[index])];
                        n[index] += u64::from(a.n);
                        sum[index] += f64::from(a.q) * f64::from(a.n);
                    }
                }
            }
            let creation = u64::from(id != 0 || !fresh_root);
            for index in 0..bound {
                let own = u64::from(table[index].n);
                assert!(
                    own >= n[index] && own <= n[index] + creation,
                    "nœud {id}, coup {index} : {own} contre {}",
                    n[index]
                );
                if own == n[index] && own > 0 {
                    let mean = sum[index] / n[index] as f64;
                    assert!((f64::from(table[index].q) - mean).abs() < 1e-3, "nœud {id}");
                }
            }
            checked += 1;
        }
        checked
    }

    #[test]
    fn opponent_amaf_matches_the_children_statistics() {
        let root = HexState::new(5).unwrap();
        let schedule = AnySchedule::MinimumMse(MinimumMse { bias: 0.02 });
        let mut engine = MctsBuilder::new(
            Rave::new(schedule, 0.0).with_grave(1_000_000),
            HexRollout::fill(),
        )
        .reuse_tree(true)
        .build()
        .unwrap();
        let mut rng = Pcg32::new(3, 17);
        let report = engine
            .search(
                &root,
                &mut SimulationBudget { simulations: 2000 },
                &mut rng,
                &CancelToken::new(),
            )
            .unwrap();
        assert!(check_opponent_amaf(engine.tree(), engine.algorithm(), true) > 5);

        // Réutilisation : la table suit la renumérotation des nœuds.
        let mut next = root.clone();
        next.play(report.best_move).unwrap();
        let reply = engine.tree().edges_of(most_visited(engine.tree(), 0));
        let reply = reply.moves[(0..reply.len()).max_by_key(|&i| reply.visits[i]).unwrap()];
        next.play(reply).unwrap();
        let report = engine
            .search_after(
                &next,
                &[report.best_move, reply],
                &mut SimulationBudget { simulations: 500 },
                &mut rng,
                &CancelToken::new(),
            )
            .unwrap();
        assert!(report.reused_visits > 20, "{}", report.reused_visits);
        assert!(check_opponent_amaf(engine.tree(), engine.algorithm(), false) > 2);
    }
}
