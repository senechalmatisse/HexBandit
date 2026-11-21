//! Arbre de recherche en arène, sans `Rc` ni `RefCell` : les nœuds
//! sont dans un `Vec` indexé par des `u32`, les arêtes dans une **structure
//! de tableaux** : une colonne par statistique.
//!
//! Les statistiques par action de l'article - N(s,a), Q(s,a) et, pour RAVE,
//! Ñ(s,a), Q̃(s,a) - sont portées par les arêtes ; N(s) est porté par le nœud.
//! Les arêtes d'un nœud occupent une plage contiguë de chaque colonne : la
//! sélection ne lit que les colonnes dont elle a besoin (N, Q et l'AMAF pour
//! MC-RAVE, soit 16 octets par arête au lieu de 28), sans branchement
//! dépendant des données, et le compilateur peut la vectoriser.
//!
//! La colonne 1/√max(N(s,a), 1) n'est tenue à jour que si l'algorithme s'en
//! sert (UCT, UCT-RAVE avec c > 0) ; sinon elle est recalculée à la demande.
//! C'est toujours une fonction de N(s,a) : les deux modes donnent les mêmes valeurs au bit près.

use game_core::{
    Game, Outcome, Player,
    rng::{Rng, shuffle},
};

/// Indice d'absence de nœud enfant.
pub const NO_NODE: u32 = u32::MAX;

/// Statistiques « all-moves-as-first » d'une action (Ñ et Q̃).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AmafStats {
    /// Ñ(s, a) : nombre de simulations AMAF.
    pub n: u32,
    /// Q̃(s, a) : valeur AMAF moyenne.
    pub q: f32,
}

/// 1/√max(N, 1), exactement comme le moteur le calcule après chaque visite.
#[inline]
#[must_use]
pub fn inv_sqrt_visits(n: u32) -> f32 {
    1.0 / (n.max(1) as f32).sqrt()
}

/// Arête (action) d'un nœud : copie des valeurs de ses colonnes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge<M, X> {
    /// Coup joué.
    pub mv: M,
    /// Nœud enfant, ou [`NO_NODE`] s'il n'a pas encore été créé.
    pub child: u32,
    /// N(s, a).
    pub n: u32,
    /// Q(s, a), du point de vue du joueur qui joue le coup.
    pub q: f32,
    /// 1/√max(N(s, a), 1), utilisé par la sélection UCB1.
    pub inv_sqrt_n: f32,
    /// Statistiques propres à l'algorithme (AMAF pour RAVE, rien pour UCT).
    pub extra: X,
}

/// Statut de résolution d'un nœud : bornes de la valeur
/// de la position pour le joueur au trait, codées sur deux bits chacune
/// (0 = défaite, 1 = nul, 2 = victoire). Sans solveur, les bornes restent [défaite, victoire].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Proof(u8);

impl Proof {
    /// Bornes inconnues : [défaite, victoire].
    pub const UNKNOWN: Proof = Proof(2 << 2);

    /// Bornes `lower ≤ upper`, en demi-points (0, 1 ou 2).
    #[must_use]
    pub const fn new(lower: u8, upper: u8) -> Proof {
        Proof((lower & 3) | ((upper & 3) << 2))
    }

    /// Valeur exacte (position résolue).
    #[must_use]
    pub const fn exact(value: u8) -> Proof {
        Proof::new(value, value)
    }

    /// Borne inférieure, en demi-points.
    #[inline]
    #[must_use]
    pub const fn lower(self) -> u8 {
        self.0 & 3
    }

    /// Borne supérieure, en demi-points.
    #[inline]
    #[must_use]
    pub const fn upper(self) -> u8 {
        (self.0 >> 2) & 3
    }

    /// Vrai si la valeur est connue exactement.
    #[inline]
    #[must_use]
    pub const fn is_solved(self) -> bool {
        self.lower() == self.upper()
    }

    /// Bornes vues par l'adversaire (valeur 2 - v).
    #[inline]
    #[must_use]
    pub const fn flipped(self) -> Proof {
        Proof::new(2 - self.upper(), 2 - self.lower())
    }

    /// Récompense correspondant à une valeur exacte (0, 0,5 ou 1).
    #[inline]
    #[must_use]
    pub fn reward(value: u8) -> f32 {
        f32::from(value) * 0.5
    }

    /// Vrai si un enfant au moins a des bornes non triviales : le solveur
    /// filtre alors la sélection de ce nœud.
    #[inline]
    #[must_use]
    pub const fn informed(self) -> bool {
        self.0 & Self::INFORMED != 0
    }

    /// Mêmes bornes, avec l'indicateur « enfant informé » donné.
    #[inline]
    #[must_use]
    pub const fn with_informed(self, informed: bool) -> Proof {
        Proof((self.0 & !Self::INFORMED) | if informed { Self::INFORMED } else { 0 })
    }

    const INFORMED: u8 = 1 << 4;
}

impl Default for Proof {
    fn default() -> Self {
        Proof::UNKNOWN
    }
}

/// Nœud (état) de l'arbre.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    /// N(s) : nombre de simulations passées par ce nœud.
    pub visits: u32,
    /// Indice de la première arête.
    pub first_edge: u32,
    /// Nombre d'arêtes (coups légaux).
    pub num_edges: u32,
    /// Joueur au trait dans cet état.
    pub player: Player,
    /// Résultat si l'état est terminal.
    pub terminal: Option<Outcome>,
    /// Bornes prouvées de la valeur pour le joueur au trait (MCTS-Solver).
    pub proof: Proof,
}

/// Arêtes d'un nœud, colonne par colonne (lecture seule).
#[derive(Debug)]
pub struct Edges<'a, M, X> {
    /// Coups.
    pub moves: &'a [M],
    /// Nœuds enfants ([`NO_NODE`] si absent).
    pub children: &'a [u32],
    /// N(s, a).
    pub visits: &'a [u32],
    /// Q(s, a).
    pub values: &'a [f32],
    /// 1/√max(N(s, a), 1) si la colonne est tenue à jour, vide sinon.
    pub inv_sqrt: &'a [f32],
    /// Statistiques propres à l'algorithme.
    pub extras: &'a [X],
}

impl<M, X> Clone for Edges<'_, M, X> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M, X> Copy for Edges<'_, M, X> {}

impl<'a, M: Copy, X: Copy> Edges<'a, M, X> {
    /// Nombre d'arêtes.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.moves.len()
    }

    /// Vrai si le nœud n'a aucune arête (position terminale).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }

    /// 1/√max(N(s, a), 1) de l'arête `i`.
    #[inline]
    #[must_use]
    pub fn inv_sqrt_n(&self, i: usize) -> f32 {
        match self.inv_sqrt.get(i) {
            Some(&v) => v,
            None => inv_sqrt_visits(self.visits[i]),
        }
    }

    /// Arête d'indice local `i`.
    #[inline]
    #[must_use]
    pub fn get(&self, i: usize) -> Edge<M, X> {
        Edge {
            mv: self.moves[i],
            child: self.children[i],
            n: self.visits[i],
            q: self.values[i],
            inv_sqrt_n: self.inv_sqrt_n(i),
            extra: self.extras[i],
        }
    }

    /// Arêtes dans l'ordre.
    #[must_use]
    pub fn iter(&self) -> EdgeIter<'a, M, X> {
        EdgeIter {
            edges: *self,
            next: 0,
        }
    }
}

/// Itérateur sur les arêtes d'un nœud (copies des valeurs).
#[derive(Clone, Debug)]
pub struct EdgeIter<'a, M, X> {
    edges: Edges<'a, M, X>,
    next: usize,
}

impl<M: Copy, X: Copy> Iterator for EdgeIter<'_, M, X> {
    type Item = Edge<M, X>;

    #[inline]
    fn next(&mut self) -> Option<Edge<M, X>> {
        if self.next < self.edges.len() {
            self.next += 1;
            Some(self.edges.get(self.next - 1))
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rest = self.edges.len() - self.next;
        (rest, Some(rest))
    }
}

impl<M: Copy, X: Copy> ExactSizeIterator for EdgeIter<'_, M, X> {}

impl<'a, M: Copy, X: Copy> IntoIterator for Edges<'a, M, X> {
    type Item = Edge<M, X>;
    type IntoIter = EdgeIter<'a, M, X>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Arène de nœuds et d'arêtes. Les capacités sont conservées entre deux
/// recherches (`clear` ne libère pas la mémoire).
#[derive(Clone, Debug)]
pub struct Tree<M, X> {
    nodes: Vec<Node>,
    moves: Vec<M>,
    children: Vec<u32>,
    visits: Vec<u32>,
    values: Vec<f32>,
    inv_sqrt: Vec<f32>,
    extras: Vec<X>,
    keep_inv_sqrt: bool,
}

impl<M: Copy, X: Copy + Default> Default for Tree<M, X> {
    fn default() -> Self {
        Tree {
            nodes: Vec::new(),
            moves: Vec::new(),
            children: Vec::new(),
            visits: Vec::new(),
            values: Vec::new(),
            inv_sqrt: Vec::new(),
            extras: Vec::new(),
            keep_inv_sqrt: true,
        }
    }
}

impl<M: Copy, X: Copy + Default> Tree<M, X> {
    /// Arbre vide (colonne 1/√N tenue à jour).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Choisit de tenir à jour la colonne 1/√N (sinon elle est recalculée à la
    /// demande). À appeler sur un arbre vide.
    pub fn set_keep_inv_sqrt(&mut self, keep: bool) {
        debug_assert!(self.moves.is_empty());
        self.keep_inv_sqrt = keep;
        if !keep {
            self.inv_sqrt = Vec::new();
        }
    }

    /// Vrai si la colonne 1/√N est tenue à jour.
    #[must_use]
    pub fn keeps_inv_sqrt(&self) -> bool {
        self.keep_inv_sqrt
    }

    /// Réserve la place de `nodes` nœuds et `edges` arêtes au total. Simple
    /// indication : si la mémoire manque, rien n'est réservé et l'arbre
    /// grandira à la demande comme d'habitude.
    pub fn reserve(&mut self, nodes: usize, edges: usize) {
        let extra = |len: usize| edges.saturating_sub(len);
        let _ = self
            .nodes
            .try_reserve(nodes.saturating_sub(self.nodes.len()));
        let _ = self.moves.try_reserve(extra(self.moves.len()));
        let _ = self.children.try_reserve(extra(self.children.len()));
        let _ = self.visits.try_reserve(extra(self.visits.len()));
        let _ = self.values.try_reserve(extra(self.values.len()));
        let _ = self.extras.try_reserve(extra(self.extras.len()));
        if self.keep_inv_sqrt {
            let _ = self.inv_sqrt.try_reserve(extra(self.inv_sqrt.len()));
        }
    }

    /// Vide l'arbre en conservant la mémoire allouée.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.moves.clear();
        self.children.clear();
        self.visits.clear();
        self.values.clear();
        self.inv_sqrt.clear();
        self.extras.clear();
    }

    /// Nombre de nœuds.
    #[inline]
    #[must_use]
    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// Nombre d'arêtes.
    #[inline]
    #[must_use]
    pub fn num_edges(&self) -> usize {
        self.moves.len()
    }

    /// Nœud d'indice `id`.
    #[inline]
    #[must_use]
    pub fn node(&self, id: u32) -> &Node {
        &self.nodes[id as usize]
    }

    /// Nœud d'indice `id`, mutable.
    #[inline]
    pub fn node_mut(&mut self, id: u32) -> &mut Node {
        &mut self.nodes[id as usize]
    }

    /// Arête d'indice global `id`.
    #[inline]
    #[must_use]
    pub fn edge(&self, id: u32) -> Edge<M, X> {
        let i = id as usize;
        Edge {
            mv: self.moves[i],
            child: self.children[i],
            n: self.visits[i],
            q: self.values[i],
            inv_sqrt_n: match self.inv_sqrt.get(i) {
                Some(&v) => v,
                None => inv_sqrt_visits(self.visits[i]),
            },
            extra: self.extras[i],
        }
    }

    /// Coup de l'arête d'indice global `id`.
    #[inline]
    #[must_use]
    pub fn edge_move(&self, id: u32) -> M {
        self.moves[id as usize]
    }

    /// Nœud enfant de l'arête d'indice global `id`.
    #[inline]
    #[must_use]
    pub fn edge_child(&self, id: u32) -> u32 {
        self.children[id as usize]
    }

    /// Rattache le nœud `child` à l'arête d'indice global `id`.
    #[inline]
    pub fn set_child(&mut self, id: u32, child: u32) {
        self.children[id as usize] = child;
    }

    fn range(&self, id: u32) -> std::ops::Range<usize> {
        let node = &self.nodes[id as usize];
        let start = node.first_edge as usize;
        start..start + node.num_edges as usize
    }

    /// Arêtes du nœud `id`.
    #[inline]
    #[must_use]
    pub fn edges_of(&self, id: u32) -> Edges<'_, M, X> {
        let r = self.range(id);
        Edges {
            moves: &self.moves[r.clone()],
            children: &self.children[r.clone()],
            visits: &self.visits[r.clone()],
            values: &self.values[r.clone()],
            inv_sqrt: if self.keep_inv_sqrt {
                &self.inv_sqrt[r.clone()]
            } else {
                &[]
            },
            extras: &self.extras[r],
        }
    }

    /// Statistiques propres à l'algorithme des arêtes du nœud `id`, mutables.
    #[inline]
    pub fn extras_of_mut(&mut self, id: u32) -> &mut [X] {
        let r = self.range(id);
        &mut self.extras[r]
    }

    /// Statistique propre à l'algorithme de l'arête d'indice global `id`, mutable.
    #[inline]
    pub fn extra_mut(&mut self, id: u32) -> &mut X {
        &mut self.extras[id as usize]
    }

    /// Fixe les statistiques d'une arête (connaissance a priori, éq. 48-51).
    pub fn set_statistics(&mut self, id: u32, n: u32, q: f32, extra: X) {
        let i = id as usize;
        self.visits[i] = n;
        self.values[i] = q;
        self.extras[i] = extra;
        if self.keep_inv_sqrt {
            self.inv_sqrt[i] = inv_sqrt_visits(n);
        }
    }

    /// Mise à jour Monte-Carlo (éq. 4-6) : N(s,a) ← N(s,a) + 1,
    /// Q(s,a) ← Q(s,a) + (z - Q(s,a)) / N(s,a).
    #[inline]
    pub fn record_visit(&mut self, id: u32, z: f32) {
        let i = id as usize;
        let n = self.visits[i] + 1;
        self.visits[i] = n;
        self.values[i] += (z - self.values[i]) / n as f32;
        if self.keep_inv_sqrt {
            self.inv_sqrt[i] = 1.0 / (n as f32).sqrt();
        }
    }

    /// Échange deux arêtes (indices globaux), toutes colonnes comprises.
    pub fn swap_edges(&mut self, a: u32, b: u32) {
        let (a, b) = (a as usize, b as usize);
        self.moves.swap(a, b);
        self.children.swap(a, b);
        self.visits.swap(a, b);
        self.values.swap(a, b);
        self.extras.swap(a, b);
        if self.keep_inv_sqrt {
            self.inv_sqrt.swap(a, b);
        }
    }

    /// Ajoute le nœud correspondant à `state` (NEWNODE de l'article) : une
    /// arête par coup légal, dans un ordre aléatoire pour que les égalités et
    /// le choix entre actions non visitées ne favorisent aucune case.
    pub fn push_node<G, R>(&mut self, state: &G, rng: &mut R, buf: &mut Vec<M>) -> u32
    where
        G: Game<Move = M>,
        R: Rng + ?Sized,
    {
        let id = self.nodes.len() as u32;
        let first_edge = self.moves.len() as u32;
        let terminal = state.outcome();
        if terminal.is_none() {
            state.legal_moves(buf);
            shuffle(rng, buf);
            let k = buf.len();
            self.moves.extend_from_slice(buf);
            let len = self.moves.len();
            self.children.resize(len, NO_NODE);
            self.visits.resize(len, 0);
            self.values.resize(len, 0.0);
            self.extras.resize(len, X::default());
            if self.keep_inv_sqrt {
                self.inv_sqrt.resize(len, 1.0);
            }
            debug_assert_eq!(len, first_edge as usize + k);
        }
        let proof = match terminal {
            None => Proof::UNKNOWN,
            Some(o) => Proof::exact((2.0 * o.reward_for(state.current_player())) as u8),
        };
        self.nodes.push(Node {
            visits: 0,
            first_edge,
            num_edges: self.moves.len() as u32 - first_edge,
            player: state.current_player(),
            terminal,
            proof,
        });
        id
    }

    /// Conserve seulement le sous-arbre de racine `root` : ses nœuds sont renumérotés dans
    /// l'ordre d'un parcours en largeur, `root` devenant le nœud 0, et l'ordre
    /// des arêtes de chaque nœud est conservé. Les colonnes sont recopiées
    /// dans des tableaux neufs de même capacité. Renvoie l'ancien numéro de
    /// chaque nœud conservé, dans le nouvel ordre.
    pub fn retain_subtree(&mut self, root: u32) -> Vec<u32> {
        let mut order = vec![root];
        let mut new_id = vec![NO_NODE; self.nodes.len()];
        new_id[root as usize] = 0;
        let mut head = 0;
        while head < order.len() {
            let r = self.range(order[head]);
            head += 1;
            for &child in &self.children[r] {
                if child != NO_NODE && new_id[child as usize] == NO_NODE {
                    new_id[child as usize] = order.len() as u32;
                    order.push(child);
                }
            }
        }
        let mut tree = Tree {
            nodes: Vec::with_capacity(self.nodes.capacity()),
            moves: Vec::with_capacity(self.moves.capacity()),
            children: Vec::with_capacity(self.children.capacity()),
            visits: Vec::with_capacity(self.visits.capacity()),
            values: Vec::with_capacity(self.values.capacity()),
            inv_sqrt: Vec::with_capacity(self.inv_sqrt.capacity()),
            extras: Vec::with_capacity(self.extras.capacity()),
            keep_inv_sqrt: self.keep_inv_sqrt,
        };
        for &old in &order {
            let r = self.range(old);
            let mut node = self.nodes[old as usize];
            node.first_edge = tree.moves.len() as u32;
            tree.nodes.push(node);
            tree.moves.extend_from_slice(&self.moves[r.clone()]);
            tree.children
                .extend(self.children[r.clone()].iter().map(|&c| {
                    if c == NO_NODE {
                        NO_NODE
                    } else {
                        new_id[c as usize]
                    }
                }));
            tree.visits.extend_from_slice(&self.visits[r.clone()]);
            tree.values.extend_from_slice(&self.values[r.clone()]);
            tree.extras.extend_from_slice(&self.extras[r.clone()]);
            if self.keep_inv_sqrt {
                tree.inv_sqrt.extend_from_slice(&self.inv_sqrt[r]);
            }
        }
        *self = tree;
        order
    }

    /// Octets occupés par une arête (somme des colonnes tenues à jour).
    #[must_use]
    pub fn bytes_per_edge(&self) -> usize {
        std::mem::size_of::<M>()
            + 3 * std::mem::size_of::<u32>()
            + std::mem::size_of::<X>()
            + if self.keep_inv_sqrt {
                std::mem::size_of::<f32>()
            } else {
                0
            }
    }

    /// Mémoire approximative occupée par l'arbre, en octets.
    #[must_use]
    pub fn memory_bytes(&self) -> usize {
        self.nodes.capacity() * std::mem::size_of::<Node>()
            + self.moves.capacity() * std::mem::size_of::<M>()
            + (self.children.capacity() + self.visits.capacity() + self.values.capacity())
                * std::mem::size_of::<u32>()
            + self.inv_sqrt.capacity() * std::mem::size_of::<f32>()
            + self.extras.capacity() * std::mem::size_of::<X>()
    }
}
