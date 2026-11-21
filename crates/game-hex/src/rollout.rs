//! Politiques de simulation propres au Hex.
//!
//! [`HexFillRollout`] exploite une propriété du Hex : une partie aléatoire
//! uniforme jouée jusqu'au bout a le même vainqueur que le plateau rempli au
//! hasard (un camp, et un seul, est connecté sur un plateau plein, et une
//! connexion ne se défait jamais). Il suffit donc de mélanger les cases vides,
//! de les attribuer alternativement aux deux joueurs et de faire **une seule**
//! vérification de connexion.
//!
//! Pour RAVE, la trace doit s'arrêter au coup qui termine réellement la partie :
//! le tirage est alors rejoué dans l'ordre avec un marquage
//! incrémental des pierres reliées à leur bord, qui s'arrête au coup gagnant.

use game_core::{
    Outcome, Player,
    rng::{Rng, shuffle},
    rollout::{RolloutPolicy, Trace, TraceMode, UniformRollout},
};

use crate::{
    Cell, HexState,
    state::{EMPTY, stone},
    topology::{EAST, Topology},
};

/// Politique « remplissage aléatoire + une vérification de connexion ».
#[derive(Clone, Debug, Default)]
pub struct HexFillRollout {
    order: Vec<u16>,
    colors: Vec<u8>,
    stack: Vec<u16>,
    marks: Vec<u32>,
    stamp: u32,
}

impl HexFillRollout {
    /// Nouvelle politique (tampons alloués au premier usage).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Vrai si Bleu relie Ouest et Est sur le plateau rempli `self.colors`.
    fn blue_connects(&mut self, state: &HexState) -> bool {
        let n = state.size();
        let topo = state.topo();
        let blue = stone(Player::P1);
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.marks.iter_mut().for_each(|m| *m = 0);
            self.stamp = 1;
        }
        let stamp = self.stamp;
        // Pile de taille fixe : chaque case y entre au plus une fois (marquée
        // avant d'être empilée), plus une place pour l'écriture inconditionnelle.
        prepare_stack(&mut self.stack, n * n);
        let stack = &mut self.stack[..];
        let mut top = 0usize;
        for row in 0..n {
            let cell = row * n;
            let push = self.colors[cell] == blue;
            self.marks[cell] = if push { stamp } else { self.marks[cell] };
            stack[top] = cell as u16;
            top += usize::from(push);
        }
        while top > 0 {
            top -= 1;
            let cell = usize::from(stack[top]);
            if topo.borders[cell] & EAST != 0 {
                return true;
            }
            // Sans branche dépendant des données : la condition est
            // imprévisible pour le processeur.
            for &nb in topo.neighbors_padded(cell) {
                let nb = usize::from(nb);
                let push = (self.colors[nb] == blue) & (self.marks[nb] != stamp);
                self.marks[nb] = if push { stamp } else { self.marks[nb] };
                stack[top] = nb as u16;
                top += usize::from(push);
            }
        }
        false
    }

    /// Rejoue le tirage dans l'ordre et renvoie le vainqueur et l'indice du
    /// coup qui termine la partie.
    ///
    /// Chaque pierre porte un bit `REACHED` si elle est reliée au premier bord
    /// de son camp par des pierres déjà posées. Une pierre posée ne peut
    /// étendre cet ensemble qu'à travers elle-même : chaque case est explorée
    /// au plus une fois, et la boucle s'arrête au coup gagnant.
    ///
    /// Le mélange de Fisher-Yates est fait au fil de l'eau : on ne tire que
    /// les cases effectivement jouées.
    fn exact_playout<R: Rng + ?Sized>(
        &mut self,
        state: &HexState,
        to_move: Player,
        rng: &mut R,
    ) -> (Player, usize) {
        const REACHED: u8 = 4;
        let n = state.size();
        let topo = state.topo();
        self.colors.clear();
        self.colors.extend_from_slice(state.cells());
        self.colors.push(EMPTY); // sentinelle n² (Topology::padded)
        // Pierres déjà posées reliées à leur premier bord.
        for player in [Player::P1, Player::P2] {
            let code = stone(player);
            self.stack.clear();
            for i in 0..n {
                let cell = if player == Player::P1 { i * n } else { i };
                if self.colors[cell] == code {
                    self.colors[cell] = code | REACHED;
                    self.stack.push(cell as u16);
                }
            }
            while let Some(cell) = self.stack.pop() {
                for &nb in topo.neighbors(usize::from(cell)) {
                    let nb = usize::from(nb);
                    if self.colors[nb] == code {
                        self.colors[nb] = code | REACHED;
                        self.stack.push(nb as u16);
                    }
                }
            }
        }
        let players = [to_move, to_move.opponent()];
        let len = self.order.len();
        prepare_stack(&mut self.stack, n * n);
        let stack = &mut self.stack[..];
        for i in 0..len {
            let j = i + rng.below((len - i) as u32) as usize;
            self.order.swap(i, j);
            let cell = usize::from(self.order[i]);
            let player = players[i & 1];
            let code = stone(player);
            let target = code | REACHED;
            let (start_bit, goal_bit) = Topology::player_borders(player);
            // Ou logique sur tous les voisins, sans sortie anticipée.
            let mut reached = topo.borders[cell] & start_bit != 0;
            for &nb in topo.neighbors_padded(cell) {
                reached |= self.colors[usize::from(nb)] == target;
            }
            if !reached {
                self.colors[cell] = code;
                continue;
            }
            self.colors[cell] = target;
            stack[0] = cell as u16;
            let mut top = 1usize;
            while top > 0 {
                top -= 1;
                let x = usize::from(stack[top]);
                if topo.borders[x] & goal_bit != 0 {
                    return (player, i);
                }
                for &nb in topo.neighbors_padded(x) {
                    let nb = usize::from(nb);
                    let push = self.colors[nb] == code;
                    self.colors[nb] |= REACHED * u8::from(push);
                    stack[top] = nb as u16;
                    top += usize::from(push);
                }
            }
        }
        debug_assert!(false, "un plateau plein a toujours un vainqueur");
        (players[0], self.order.len().saturating_sub(1))
    }
}

/// Prépare une pile de taille fixe pour un parcours sur `cells` cases : chaque
/// case y entre au plus une fois, et l'empilement sans branche écrit toujours
/// une case au-delà du sommet.
fn prepare_stack(stack: &mut Vec<u16>, cells: usize) {
    if stack.len() < cells + 1 {
        stack.resize(cells + 1, 0);
    }
}

impl RolloutPolicy<HexState> for HexFillRollout {
    fn rollout<R: Rng + ?Sized>(
        &mut self,
        state: &mut HexState,
        rng: &mut R,
        trace: &mut Trace<Cell>,
    ) -> Outcome {
        if let Some(outcome) = game_core::Game::outcome(state) {
            return outcome;
        }
        let to_move = game_core::Game::current_player(state);
        let opponent = to_move.opponent();
        let n2 = state.size() * state.size();
        if self.marks.len() != n2 + 1 {
            self.marks.clear();
            self.marks.resize(n2 + 1, 0);
            self.stamp = 0;
        }
        self.order.clear();
        self.order.extend_from_slice(state.empties());
        if trace.mode() == TraceMode::Full {
            let (winner, end) = self.exact_playout(state, to_move, rng);
            let players = [to_move, opponent];
            trace.extend(
                self.order[..=end]
                    .iter()
                    .enumerate()
                    .map(|(i, &c)| (Cell(c), players[i & 1])),
            );
            return Outcome::Win(winner);
        }
        shuffle(rng, &mut self.order);
        self.colors.clear();
        self.colors.extend_from_slice(state.cells());
        self.colors.push(EMPTY); // sentinelle n² (Topology::padded)
        let (mine, theirs) = (stone(to_move), stone(opponent));
        for (i, &cell) in self.order.iter().enumerate() {
            debug_assert_eq!(self.colors[usize::from(cell)], EMPTY);
            self.colors[usize::from(cell)] = if i % 2 == 0 { mine } else { theirs };
        }
        let winner = if self.blue_connects(state) {
            Player::P1
        } else {
            Player::P2
        };
        if trace.mode() == TraceMode::FirstMove {
            if let Some(&first) = self.order.first() {
                trace.record(Cell(first), to_move);
            }
        }
        Outcome::Win(winner)
    }
}

/// Choix, à l'exécution, de la politique de simulation au Hex.
#[derive(Clone, Debug)]
pub enum HexRollout {
    /// Coups uniformes joués un par un (politique générique).
    Uniform(UniformRollout<HexState>),
    /// Remplissage aléatoire et une seule vérification de connexion.
    Fill(HexFillRollout),
}

impl HexRollout {
    /// Politique uniforme générique.
    #[must_use]
    pub fn uniform() -> Self {
        HexRollout::Uniform(UniformRollout::new())
    }

    /// Politique de remplissage optimisée.
    #[must_use]
    pub fn fill() -> Self {
        HexRollout::Fill(HexFillRollout::new())
    }
}

impl RolloutPolicy<HexState> for HexRollout {
    #[inline]
    fn rollout<R: Rng + ?Sized>(
        &mut self,
        state: &mut HexState,
        rng: &mut R,
        trace: &mut Trace<Cell>,
    ) -> Outcome {
        match self {
            HexRollout::Uniform(p) => p.rollout(state, rng, trace),
            HexRollout::Fill(p) => p.rollout(state, rng, trace),
        }
    }
}
