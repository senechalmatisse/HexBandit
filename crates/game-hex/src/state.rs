//! Position de Hex : plateau, joueur au trait et détection incrémentale de la
//! victoire par union-find avec quatre bords virtuels.

use game_core::{AmafGame, Game, GameError, Outcome, Player, decorators::PieGame, rng::Rng};

use crate::{
    BLUE, Cell, MAX_SIZE, MIN_SIZE,
    notation::format_cell,
    topology::{Topology, topology},
};

/// Contenu d'une case vide.
pub(crate) const EMPTY: u8 = 0;

/// Code de pierre d'un joueur dans le tableau des cases (1 pour `P1`, 2 pour `P2`),
/// calculé sans branchement : la simulation l'appelle à chaque coup, avec des joueurs qui alternent.
#[inline]
pub(crate) const fn stone(player: Player) -> u8 {
    player.index() as u8 + 1
}

/// Joueur propriétaire d'un code de pierre.
#[inline]
pub(crate) const fn owner(code: u8) -> Option<Player> {
    match code {
        1 => Some(Player::P1),
        2 => Some(Player::P2),
        _ => None,
    }
}

/// Position de Hex sur un plateau `n × n` (2 ≤ n ≤ 19).
///
/// Bleu ([`BLUE`]) relie Ouest (colonne 0) et Est (colonne n - 1) ; Rouge
/// ([`RED`]) relie Nord (ligne 0) et Sud (ligne n - 1). Le clonage ne fait
/// qu'une copie de quelques tableaux, et `clone_from` réutilise les tampons existants.
#[derive(Debug)]
pub struct HexState {
    topo: &'static Topology,
    cells: Vec<u8>,
    parent: Vec<u16>,
    rank: Vec<u8>,
    empties: Vec<u16>,
    empty_pos: Vec<u16>,
    to_move: Player,
    first_player: Player,
    winner: Option<Player>,
    ply: u32,
}

impl Clone for HexState {
    fn clone(&self) -> Self {
        HexState {
            topo: self.topo,
            cells: self.cells.clone(),
            parent: self.parent.clone(),
            rank: self.rank.clone(),
            empties: self.empties.clone(),
            empty_pos: self.empty_pos.clone(),
            to_move: self.to_move,
            first_player: self.first_player,
            winner: self.winner,
            ply: self.ply,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.topo = source.topo;
        self.cells.clone_from(&source.cells);
        self.parent.clone_from(&source.parent);
        self.rank.clone_from(&source.rank);
        self.empties.clone_from(&source.empties);
        self.empty_pos.clone_from(&source.empty_pos);
        self.to_move = source.to_move;
        self.first_player = source.first_player;
        self.winner = source.winner;
        self.ply = source.ply;
    }
}

impl PartialEq for HexState {
    /// Deux positions sont égales si elles ont les mêmes pierres, le même
    /// joueur au trait et le même premier joueur (l'ordre des coups est ignoré).
    fn eq(&self, other: &Self) -> bool {
        self.cells == other.cells
            && self.to_move == other.to_move
            && self.first_player == other.first_player
            && self.winner == other.winner
    }
}

impl Eq for HexState {}

impl HexState {
    /// Plateau vide de taille `size`, Bleu commence.
    ///
    /// # Errors
    ///
    /// [`GameError::InvalidParameter`] si la taille n'est pas dans `[2, 19]`.
    pub fn new(size: usize) -> Result<Self, GameError> {
        Self::with_first_player(size, BLUE)
    }

    /// Plateau vide de taille `size`, avec le premier joueur indiqué.
    ///
    /// # Errors
    ///
    /// [`GameError::InvalidParameter`] si la taille n'est pas dans `[2, 19]`.
    pub fn with_first_player(size: usize, first_player: Player) -> Result<Self, GameError> {
        if !(MIN_SIZE..=MAX_SIZE).contains(&size) {
            return Err(GameError::InvalidParameter {
                name: "size",
                reason: format!("{size} hors de [{MIN_SIZE}, {MAX_SIZE}]"),
            });
        }
        let n2 = size * size;
        let mut state = HexState {
            topo: topology(size),
            cells: vec![EMPTY; n2],
            parent: Vec::new(),
            rank: Vec::new(),
            empties: (0..n2 as u16).collect(),
            empty_pos: (0..n2 as u16).collect(),
            to_move: first_player,
            first_player,
            winner: None,
            ply: 0,
        };
        state.reset_union_find();
        Ok(state)
    }

    /// Reconstruit une position à partir d'une liste de coups.
    ///
    /// # Errors
    ///
    /// Taille invalide ou coup illégal dans la liste.
    pub fn from_moves(
        size: usize,
        first_player: Player,
        moves: &[Cell],
    ) -> Result<Self, GameError> {
        let mut state = Self::with_first_player(size, first_player)?;
        for &mv in moves {
            state.play(mv)?;
        }
        Ok(state)
    }

    /// Taille du côté du plateau.
    #[inline]
    #[must_use]
    pub fn size(&self) -> usize {
        self.topo.n
    }

    /// Joueur qui a commencé la partie.
    #[must_use]
    pub fn first_player(&self) -> Player {
        self.first_player
    }

    /// Vainqueur, si la partie est terminée.
    #[must_use]
    pub fn winner(&self) -> Option<Player> {
        self.winner
    }

    /// Propriétaire de la pierre en `cell` (None si vide ou hors plateau).
    #[must_use]
    pub fn stone_at(&self, cell: Cell) -> Option<Player> {
        self.cells.get(usize::from(cell.0)).and_then(|&c| owner(c))
    }

    /// Propriétaire de la pierre en ligne `row`, colonne `col`.
    #[must_use]
    pub fn stone_at_rc(&self, row: usize, col: usize) -> Option<Player> {
        let n = self.size();
        if row >= n || col >= n {
            return None;
        }
        owner(self.cells[row * n + col])
    }

    /// Nombre de cases vides.
    #[must_use]
    pub fn num_empty(&self) -> usize {
        self.empties.len()
    }

    /// Nombre de pierres de chaque joueur `[P1, P2]`.
    #[must_use]
    pub fn stone_counts(&self) -> [usize; 2] {
        let mut counts = [0usize; 2];
        for &c in &self.cells {
            if let Some(p) = owner(c) {
                counts[p.index()] += 1;
            }
        }
        counts
    }

    // accès internes utilisés par la politique de remplissage

    #[inline]
    pub(crate) fn topo(&self) -> &'static Topology {
        self.topo
    }

    #[inline]
    pub(crate) fn cells(&self) -> &[u8] {
        &self.cells
    }

    #[inline]
    pub(crate) fn empties(&self) -> &[u16] {
        &self.empties
    }

    // union-find

    /// Indices des quatre bords virtuels.
    #[inline]
    pub(crate) fn edge_nodes(n: usize, player: Player) -> (usize, usize) {
        let n2 = n * n;
        match player {
            Player::P1 => (n2, n2 + 1),     // Ouest, Est
            Player::P2 => (n2 + 2, n2 + 3), // Nord, Sud
        }
    }

    fn reset_union_find(&mut self) {
        let total = self.cells.len() + 4;
        self.parent.clear();
        self.parent.extend(0..total as u16);
        self.rank.clear();
        self.rank.resize(total, 0);
    }

    #[inline]
    fn find(&mut self, mut x: usize) -> usize {
        loop {
            let p = self.parent[x] as usize;
            if p == x {
                return x;
            }
            let gp = self.parent[p];
            self.parent[x] = gp;
            x = gp as usize;
        }
    }

    #[inline]
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        match self.rank[ra].cmp(&self.rank[rb]) {
            std::cmp::Ordering::Less => self.parent[ra] = rb as u16,
            std::cmp::Ordering::Greater => self.parent[rb] = ra as u16,
            std::cmp::Ordering::Equal => {
                self.parent[rb] = ra as u16;
                self.rank[ra] += 1;
            }
        }
    }

    /// Pose une pierre (sans contrôle) et met à jour l'union-find et le vainqueur.
    fn place(&mut self, cell: usize, player: Player) {
        let n = self.size();
        let code = stone(player);
        self.cells[cell] = code;
        // Retrait de la liste des cases vides (échange avec la dernière).
        let pos = self.empty_pos[cell] as usize;
        if let Some(last) = self.empties.pop() {
            if pos < self.empties.len() {
                self.empties[pos] = last;
                self.empty_pos[last as usize] = pos as u16;
            }
        }
        let topo = self.topo;
        for &nb in topo.neighbors(cell) {
            if self.cells[nb as usize] == code {
                self.union(cell, nb as usize);
            }
        }
        let (edge_a, edge_b) = Self::edge_nodes(n, player);
        let (bit_a, bit_b) = Topology::player_borders(player);
        let borders = topo.borders[cell];
        if borders & bit_a != 0 {
            self.union(cell, edge_a);
        }
        if borders & bit_b != 0 {
            self.union(cell, edge_b);
        }
        if self.find(edge_a) == self.find(edge_b) {
            self.winner = Some(player);
        }
    }

    /// Reconstruit entièrement l'union-find, la liste des cases vides et le
    /// vainqueur à partir du tableau des cases. Renvoie, pour chaque joueur,
    /// s'il relie ses deux bords.
    pub(crate) fn rebuild_from_cells(&mut self) -> [bool; 2] {
        let cells = std::mem::take(&mut self.cells);
        let n2 = cells.len();
        self.cells = vec![EMPTY; n2];
        self.empties = (0..n2 as u16).collect();
        self.empty_pos = (0..n2 as u16).collect();
        self.winner = None;
        self.reset_union_find();
        let mut connected = [false; 2];
        for (cell, &code) in cells.iter().enumerate() {
            if let Some(p) = owner(code) {
                self.place(cell, p);
                if self.winner == Some(p) {
                    connected[p.index()] = true;
                }
                self.winner = None;
            }
        }
        self.winner = match connected {
            [true, false] => Some(Player::P1),
            [false, true] => Some(Player::P2),
            _ => None,
        };
        debug_assert_eq!(self.cells, cells);
        connected
    }

    /// Construit une position quelconque (pierres, joueur au trait, premier joueur).
    pub(crate) fn from_parts(
        size: usize,
        cells: Vec<u8>,
        to_move: Player,
        first_player: Player,
    ) -> Result<Self, GameError> {
        let mut state = Self::with_first_player(size, first_player)?;
        if cells.len() != size * size {
            return Err(GameError::InvalidParameter {
                name: "cells",
                reason: "nombre de cases incohérent".to_owned(),
            });
        }
        state.cells = cells;
        if state.rebuild_from_cells() == [true, true] {
            return Err(GameError::InvalidParameter {
                name: "cells",
                reason: "les deux joueurs ne peuvent pas être connectés".to_owned(),
            });
        }
        state.to_move = to_move;
        let counts = state.stone_counts();
        state.ply = (counts[0] + counts[1]) as u32;
        Ok(state)
    }

    /// Chemin gagnant minimal (en nombre de pierres) du vainqueur, d'un bord à
    /// l'autre, ou `None` si la partie n'est pas terminée.
    #[must_use]
    pub fn winning_path(&self) -> Option<Vec<Cell>> {
        let winner = self.winner?;
        let n = self.size();
        let code = stone(winner);
        let starts = |cell: usize| match winner {
            Player::P1 => cell % n == 0,
            Player::P2 => cell / n == 0,
        };
        let goal = |cell: usize| match winner {
            Player::P1 => cell % n == n - 1,
            Player::P2 => cell / n == n - 1,
        };
        let mut prev = vec![u16::MAX; n * n];
        let mut seen = vec![false; n * n];
        let mut queue = std::collections::VecDeque::new();
        for (cell, &c) in self.cells.iter().enumerate() {
            if c == code && starts(cell) {
                seen[cell] = true;
                queue.push_back(cell);
            }
        }
        while let Some(cell) = queue.pop_front() {
            if goal(cell) {
                let mut path = vec![Cell(cell as u16)];
                let mut cur = cell;
                while prev[cur] != u16::MAX {
                    cur = prev[cur] as usize;
                    path.push(Cell(cur as u16));
                }
                path.reverse();
                return Some(path);
            }
            for &nb in self.topo.neighbors(cell) {
                let nb = nb as usize;
                if !seen[nb] && self.cells[nb] == code {
                    seen[nb] = true;
                    prev[nb] = cell as u16;
                    queue.push_back(nb);
                }
            }
        }
        None
    }
}

impl Game for HexState {
    type Move = Cell;

    #[inline]
    fn current_player(&self) -> Player {
        self.to_move
    }

    /// Coups légaux, dans l'ordre (déterministe) de la liste interne des cases
    /// vides : copie directe, sans balayage du plateau.
    fn legal_moves(&self, out: &mut Vec<Cell>) {
        out.clear();
        if self.winner.is_some() {
            return;
        }
        out.extend(self.empties.iter().map(|&c| Cell(c)));
    }

    fn play(&mut self, mv: Cell) -> Result<(), GameError> {
        if self.winner.is_some() {
            return Err(GameError::GameOver);
        }
        let n = self.size();
        let cell = usize::from(mv.0);
        if cell >= n * n {
            return Err(GameError::OutOfBounds {
                mv: format!("#{}", mv.0),
            });
        }
        if self.cells[cell] != EMPTY {
            return Err(GameError::Occupied {
                mv: format_cell(mv, n),
            });
        }
        self.place(cell, self.to_move);
        self.to_move = self.to_move.opponent();
        self.ply += 1;
        Ok(())
    }

    #[inline]
    fn outcome(&self) -> Option<Outcome> {
        self.winner.map(Outcome::Win)
    }

    #[inline]
    fn ply(&self) -> u32 {
        self.ply
    }

    #[inline]
    fn random_legal_move<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        _buf: &mut Vec<Cell>,
    ) -> Option<Cell> {
        if self.winner.is_some() || self.empties.is_empty() {
            return None;
        }
        Some(Cell(
            self.empties[rng.below(self.empties.len() as u32) as usize],
        ))
    }
}

impl AmafGame for HexState {
    #[inline]
    fn move_index(&self, mv: Cell) -> usize {
        usize::from(mv.0)
    }

    #[inline]
    fn move_index_bound(&self) -> usize {
        self.cells.len()
    }
}

impl PieGame for HexState {
    /// Échange (forme « swap pieces ») : la pierre du premier coup, en `(r, c)`,
    /// est remplacée par une pierre de l'adversaire en `(c, r)` ; le premier
    /// joueur rejoue. La transposition échange les bords Est-Ouest et
    /// Nord-Sud, ce qui rend la position équivalente à une ouverture adverse.
    fn apply_swap(&mut self) -> Result<(), GameError> {
        if self.winner.is_some() {
            return Err(GameError::GameOver);
        }
        if self.ply != 1 {
            return Err(GameError::IllegalMove {
                mv: "swap".to_owned(),
                reason: "l'échange n'est possible qu'après le premier coup".to_owned(),
            });
        }
        let n = self.size();
        let Some(cell) = self.cells.iter().position(|&c| c != EMPTY) else {
            return Err(GameError::IllegalMove {
                mv: "swap".to_owned(),
                reason: "aucune pierre à échanger".to_owned(),
            });
        };
        let Some(mover) = owner(self.cells[cell]) else {
            return Err(GameError::IllegalMove {
                mv: "swap".to_owned(),
                reason: "case vide".to_owned(),
            });
        };
        let (row, col) = (cell / n, cell % n);
        let mut cells = vec![EMPTY; n * n];
        cells[col * n + row] = stone(mover.opponent());
        self.cells = cells;
        self.rebuild_from_cells();
        self.to_move = mover;
        self.ply = 2;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RED, notation::parse_cell};

    fn cells(n: usize, names: &[&str]) -> Vec<Cell> {
        names.iter().map(|s| parse_cell(s, n).unwrap()).collect()
    }

    #[test]
    fn rejects_invalid_sizes_and_moves() {
        assert!(HexState::new(1).is_err());
        assert!(HexState::new(20).is_err());
        let mut s = HexState::new(3).unwrap();
        assert!(matches!(
            s.play(Cell(9)),
            Err(GameError::OutOfBounds { .. })
        ));
        s.play(Cell(4)).unwrap();
        assert!(matches!(s.play(Cell(4)), Err(GameError::Occupied { .. })));
        assert_eq!(s.ply(), 1);
        assert_eq!(s.current_player(), RED);
    }

    #[test]
    fn blue_wins_with_a_row() {
        // Bleu relie Ouest-Est sur la première ligne, Rouge joue ailleurs.
        let moves = cells(3, &["a1", "a3", "b1", "b3", "c1"]);
        let s = HexState::from_moves(3, BLUE, &moves).unwrap();
        assert_eq!(s.winner(), Some(BLUE));
        assert_eq!(s.outcome(), Some(Outcome::Win(BLUE)));
        let mut buf = Vec::new();
        s.legal_moves(&mut buf);
        assert!(buf.is_empty());
        let path = s.winning_path().unwrap();
        assert_eq!(path, cells(3, &["a1", "b1", "c1"]));
    }

    #[test]
    fn red_wins_with_a_column_using_diagonal_adjacency() {
        // Rouge relie Nord-Sud via (0,2) -> (1,1) -> (2,0) : voisins (+1, -1).
        let moves = cells(3, &["c1", "a1", "b2", "a2", "a3"]);
        let s = HexState::from_moves(3, RED, &moves).unwrap();
        assert_eq!(s.winner(), Some(RED));
        assert_eq!(s.winning_path().unwrap().len(), 3);
    }

    #[test]
    fn swap_transposes_and_changes_owner() {
        let mut s = HexState::new(5).unwrap();
        s.play(parse_cell("b1", 5).unwrap()).unwrap(); // (r=0, c=1)
        s.apply_swap().unwrap();
        assert_eq!(s.stone_at_rc(1, 0), Some(RED));
        assert_eq!(s.stone_at_rc(0, 1), None);
        assert_eq!(s.current_player(), BLUE);
        assert_eq!(s.ply(), 2);
        assert!(s.apply_swap().is_err());
    }

    #[test]
    fn clone_from_reuses_and_matches() {
        let mut a = HexState::new(7).unwrap();
        let b = HexState::from_moves(7, BLUE, &cells(7, &["d4", "c3", "e5"])).unwrap();
        a.clone_from(&b);
        assert_eq!(a, b);
        assert_eq!(a.num_empty(), 46);
    }
}
