//! Chameleon : variante du Hex.
//!
//! # Règles
//!
//! * Deux joueurs jouent à tour de rôle sur le plateau en losange du Hex ; à
//!   son tour, un joueur pose une pierre **de l'une ou l'autre couleur**
//!   (bleue ou rouge) sur une case vide : les pierres n'appartiennent à
//!   personne.
//! * Le joueur Bleu ([`game_hex::BLUE`], « Horz ») gagne dès qu'une chaîne de
//!   pierres **d'une même couleur** relie les bords Ouest et Est ; le joueur
//!   Rouge ([`game_hex::RED`], « Vert ») dès qu'une chaîne d'une même couleur
//!   relie les bords Nord et Sud.
//! * Si un coup crée à la fois une chaîne Ouest-Est et une chaîne Nord-Sud
//!   (chaîne reliant deux coins opposés, par exemple), **le joueur qui a joué
//!   ce coup gagne**.
//! * Règle du gâteau facultative (décorateur [`game_core::decorators::SwapRule`]) :
//!   l'échange transpose la première pierre, qui garde sa couleur.
//!
//! Un plateau plein contient toujours une chaîne gagnante (propriété du Hex
//! appliquée à la coloration des pierres) : il n'y a pas de match nul.
//!
//! # Coups
//!
//! Un coup est composé (case, couleur) : `c4b` pose une pierre bleue en c4,
//! `c4r` une pierre rouge. Contrairement au Quarto, un coup a le même sens
//! dans toutes les positions : le jeu implémente [`AmafGame`] avec l'indice
//! `case × 2 + couleur`, et MC-RAVE s'y applique sans modifier le crate `mcts`.
#![forbid(unsafe_code)]

use game_core::{AmafGame, Game, GameError, Outcome, Player, decorators::PieGame, rng::Rng};
use game_hex::{
    BLUE, Cell, Geometry, RED,
    notation::{format_cell, parse_cell, parse_player},
};

/// Case vide.
const EMPTY: u8 = 0;

/// Code d'une pierre de la couleur `color` (1 bleue, 2 rouge).
#[inline]
const fn stone(color: Player) -> u8 {
    color.index() as u8 + 1
}

/// Coup : une pierre de couleur `color` en `cell`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChameleonMove {
    /// Case.
    pub cell: Cell,
    /// Couleur de la pierre ([`BLUE`] ou [`RED`]), indépendante du joueur.
    pub color: Player,
}

/// Union-find des pierres d'une couleur. Chaque chaîne retient les bords
/// qu'elle touche (masque Ouest, Est, Nord, Sud, fusionné à chaque union) :
/// une chaîne de cette couleur peut gagner dans les deux directions.
///
/// Les bords ne sont pas des nœuds virtuels comme au Hex : deux chaînes qui
/// touchent toutes deux le Nord seraient sinon réunies par lui, et une chaîne
/// Ouest-Nord et une chaîne Nord-Est passeraient à tort pour une chaîne Ouest-Est.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Connectivity {
    parent: Vec<u16>,
    borders: Vec<u8>,
}

impl Connectivity {
    fn new(cells: usize) -> Self {
        Connectivity {
            parent: (0..cells as u16).collect(),
            borders: vec![0; cells],
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        loop {
            let p = usize::from(self.parent[x]);
            if p == x {
                return x;
            }
            let gp = self.parent[p];
            self.parent[x] = gp;
            x = usize::from(gp);
        }
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            // Le plus petit indice devient la racine : déterministe et suffisant
            // (compression de chemin à la recherche).
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            self.parent[hi] = lo as u16;
            self.borders[lo] |= self.borders[hi];
        }
    }
}

/// Bords Ouest et Est.
const WEST_EAST: u8 = Geometry::WEST | Geometry::EAST;
/// Bords Nord et Sud.
const NORTH_SOUTH: u8 = Geometry::NORTH | Geometry::SOUTH;

/// Position de Chameleon sur un plateau `n × n` (2 ≤ n ≤ 19).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChameleonState {
    geometry: GeometryKey,
    cells: Vec<u8>,
    links: [Connectivity; 2],
    empties: Vec<u16>,
    empty_pos: Vec<u16>,
    to_move: Player,
    first_player: Player,
    outcome: Option<Outcome>,
    ply: u32,
}

/// Géométrie (comparée par la taille seulement).
#[derive(Clone, Copy, Debug)]
struct GeometryKey(Geometry);

impl PartialEq for GeometryKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.size() == other.0.size()
    }
}

impl Eq for GeometryKey {}

impl ChameleonState {
    /// Plateau vide, Bleu (Horz) commence.
    ///
    /// # Errors
    ///
    /// [`GameError::InvalidParameter`] si la taille n'est pas dans `[2, 19]`.
    pub fn new(size: usize) -> Result<Self, GameError> {
        Self::with_first_player(size, BLUE)
    }

    /// Plateau vide avec le premier joueur indiqué.
    ///
    /// # Errors
    ///
    /// [`GameError::InvalidParameter`] si la taille n'est pas dans `[2, 19]`.
    pub fn with_first_player(size: usize, first_player: Player) -> Result<Self, GameError> {
        let geometry = Geometry::new(size)?;
        let n2 = size * size;
        Ok(ChameleonState {
            geometry: GeometryKey(geometry),
            cells: vec![EMPTY; n2],
            links: [Connectivity::new(n2), Connectivity::new(n2)],
            empties: (0..n2 as u16).collect(),
            empty_pos: (0..n2 as u16).collect(),
            to_move: first_player,
            first_player,
            outcome: None,
            ply: 0,
        })
    }

    /// Côté du plateau.
    #[must_use]
    pub fn size(&self) -> usize {
        self.geometry.0.size()
    }

    /// Joueur qui a commencé.
    #[must_use]
    pub fn first_player(&self) -> Player {
        self.first_player
    }

    /// Couleur de la pierre en `cell`, si la case est occupée.
    #[must_use]
    pub fn stone_at(&self, cell: Cell) -> Option<Player> {
        match self.cells.get(usize::from(cell.0)) {
            Some(1) => Some(BLUE),
            Some(2) => Some(RED),
            _ => None,
        }
    }

    /// Nombre de cases vides.
    #[must_use]
    pub fn num_empty(&self) -> usize {
        self.empties.len()
    }

    /// Pose une pierre (sans contrôle) et renvoie les directions reliées par
    /// la chaîne de cette couleur : (Ouest-Est, Nord-Sud).
    fn place(&mut self, cell: usize, color: Player) -> (bool, bool) {
        let geometry = self.geometry.0;
        let code = stone(color);
        self.cells[cell] = code;
        let pos = usize::from(self.empty_pos[cell]);
        if let Some(last) = self.empties.pop() {
            if pos < self.empties.len() {
                self.empties[pos] = last;
                self.empty_pos[usize::from(last)] = pos as u16;
            }
        }
        let links = &mut self.links[color.index()];
        links.borders[cell] = geometry.borders(cell);
        for &nb in geometry.neighbors(cell) {
            if self.cells[usize::from(nb)] == code {
                links.union(cell, usize::from(nb));
            }
        }
        // Seule la chaîne de la nouvelle pierre a changé.
        let root = links.find(cell);
        let touched = links.borders[root];
        (
            touched & WEST_EAST == WEST_EAST,
            touched & NORTH_SOUTH == NORTH_SOUTH,
        )
    }

    /// Construit une position quelconque (pierres, joueur au trait).
    ///
    /// # Errors
    ///
    /// Taille invalide ou position impossible (les deux directions reliées).
    pub fn from_cells(
        size: usize,
        cells: &[Option<Player>],
        to_move: Player,
    ) -> Result<Self, GameError> {
        if cells.len() != size * size {
            return Err(GameError::InvalidParameter {
                name: "cells",
                reason: "nombre de cases incohérent".to_owned(),
            });
        }
        let stones = cells.iter().filter(|c| c.is_some()).count();
        // Les pierres n'appartiennent à personne : le premier joueur se
        // déduit de la parité du nombre de coups joués.
        let first = if stones % 2 == 0 {
            to_move
        } else {
            to_move.opponent()
        };
        let mut state = Self::with_first_player(size, first)?;
        let (mut west_east, mut north_south) = (false, false);
        for (cell, color) in cells.iter().enumerate() {
            if let Some(color) = *color {
                let (we, ns) = state.place(cell, color);
                west_east |= we;
                north_south |= ns;
            }
        }
        state.outcome = match (west_east, north_south) {
            (true, true) => {
                return Err(GameError::InvalidParameter {
                    name: "cells",
                    reason: "les deux directions sont reliées".to_owned(),
                });
            }
            (true, false) => Some(Outcome::Win(BLUE)),
            (false, true) => Some(Outcome::Win(RED)),
            (false, false) => None,
        };
        state.to_move = to_move;
        state.ply = stones as u32;
        Ok(state)
    }

    /// Position au format du Hex (`.` vide, `b` pierre bleue, `r` pierre
    /// rouge, lignes séparées par `/`), suivie du joueur au trait.
    #[must_use]
    pub fn to_position(&self) -> String {
        let n = self.size();
        let mut out = String::with_capacity(n * (n + 1) + 2);
        for row in 0..n {
            if row > 0 {
                out.push('/');
            }
            for col in 0..n {
                out.push(match self.stone_at(Cell::from_rc(row, col, n)) {
                    None => '.',
                    Some(Player::P1) => 'b',
                    Some(Player::P2) => 'r',
                });
            }
        }
        out.push(' ');
        out.push(if self.to_move == BLUE { 'b' } else { 'r' });
        out
    }

    /// Lit une position au format de [`ChameleonState::to_position`].
    ///
    /// # Errors
    ///
    /// [`GameError::Parse`] si le texte est mal formé ou la position impossible.
    pub fn from_position(text: &str) -> Result<Self, GameError> {
        let err = |reason: &str| GameError::Parse {
            input: text.to_owned(),
            reason: reason.to_owned(),
        };
        let mut parts = text.split_whitespace();
        let board = parts.next().ok_or_else(|| err("position vide"))?;
        let to_move = parse_player(
            parts
                .next()
                .ok_or_else(|| err("joueur au trait manquant"))?,
        )?;
        if parts.next().is_some() {
            return Err(err("texte en trop"));
        }
        let rows: Vec<&str> = board.split('/').collect();
        let n = rows.len();
        let mut cells = Vec::with_capacity(n * n);
        for row in &rows {
            if row.chars().count() != n {
                return Err(err("le plateau doit être carré"));
            }
            for ch in row.chars() {
                cells.push(match ch.to_ascii_lowercase() {
                    '.' => None,
                    'b' => Some(BLUE),
                    'r' => Some(RED),
                    _ => return Err(err("caractère de case inconnu")),
                });
            }
        }
        Self::from_cells(n, &cells, to_move).map_err(|e| err(&e.to_string()))
    }
}

impl Game for ChameleonState {
    type Move = ChameleonMove;

    #[inline]
    fn current_player(&self) -> Player {
        self.to_move
    }

    fn legal_moves(&self, out: &mut Vec<ChameleonMove>) {
        out.clear();
        if self.outcome.is_some() {
            return;
        }
        for &c in &self.empties {
            for color in [BLUE, RED] {
                out.push(ChameleonMove {
                    cell: Cell(c),
                    color,
                });
            }
        }
    }

    fn play(&mut self, mv: ChameleonMove) -> Result<(), GameError> {
        if self.outcome.is_some() {
            return Err(GameError::GameOver);
        }
        let n = self.size();
        let cell = usize::from(mv.cell.0);
        if cell >= n * n {
            return Err(GameError::OutOfBounds {
                mv: format!("#{}", mv.cell.0),
            });
        }
        if self.cells[cell] != EMPTY {
            return Err(GameError::Occupied {
                mv: format_move(mv, n),
            });
        }
        let mover = self.to_move;
        self.outcome = match self.place(cell, mv.color) {
            (true, true) => Some(Outcome::Win(mover)),
            (true, false) => Some(Outcome::Win(BLUE)),
            (false, true) => Some(Outcome::Win(RED)),
            (false, false) => None,
        };
        self.to_move = mover.opponent();
        self.ply += 1;
        Ok(())
    }

    #[inline]
    fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    #[inline]
    fn ply(&self) -> u32 {
        self.ply
    }

    /// Coup uniforme parmi les 2 × (cases vides) coups légaux, en un seul tirage.
    #[inline]
    fn random_legal_move<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        _buf: &mut Vec<ChameleonMove>,
    ) -> Option<ChameleonMove> {
        if self.outcome.is_some() || self.empties.is_empty() {
            return None;
        }
        let k = rng.below(2 * self.empties.len() as u32) as usize;
        Some(ChameleonMove {
            cell: Cell(self.empties[k >> 1]),
            color: if k & 1 == 0 { BLUE } else { RED },
        })
    }
}

impl AmafGame for ChameleonState {
    /// `case × 2 + couleur` : une pierre bleue en c4 et une pierre rouge en c4
    /// sont deux coups distincts, chacun avec sa propre valeur AMAF.
    #[inline]
    fn move_index(&self, mv: ChameleonMove) -> usize {
        usize::from(mv.cell.0) * 2 + mv.color.index()
    }

    #[inline]
    fn move_index_bound(&self) -> usize {
        2 * self.cells.len()
    }
}

impl PieGame for ChameleonState {
    /// Échange : la première pierre, en `(r, c)`, est déplacée en `(c, r)` en
    /// gardant sa couleur. La transposition échange les directions Ouest-Est
    /// et Nord-Sud : le second joueur hérite de l'ouverture adverse. Le premier joueur rejoue.
    fn apply_swap(&mut self) -> Result<(), GameError> {
        if self.outcome.is_some() {
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
        let color = if self.cells[cell] == stone(BLUE) {
            BLUE
        } else {
            RED
        };
        let mover = self.to_move.opponent();
        let (row, col) = (cell / n, cell % n);
        let mut swapped = Self::with_first_player(n, self.first_player)?;
        swapped.place(col * n + row, color);
        swapped.to_move = mover;
        swapped.ply = 2;
        *self = swapped;
        Ok(())
    }
}

/// Notation d'un coup : case puis couleur (`c4b`, `c4r`).
#[must_use]
pub fn format_move(mv: ChameleonMove, size: usize) -> String {
    let color = if mv.color == BLUE { 'b' } else { 'r' };
    format!("{}{color}", format_cell(mv.cell, size))
}

/// Lecture d'un coup : `c4b`, `c4r`, ou `c4:blue`, `c4:rouge`…
///
/// # Errors
///
/// [`GameError::Parse`] si le texte est mal formé ou hors du plateau.
pub fn parse_move(text: &str, size: usize) -> Result<ChameleonMove, GameError> {
    let err = |reason: &str| GameError::Parse {
        input: text.to_owned(),
        reason: reason.to_owned(),
    };
    let trimmed = text.trim();
    let (cell, color) = match trimmed.split_once(':') {
        Some((cell, color)) => (cell, color),
        None => {
            let split = trimmed
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .ok_or_else(|| err("coup vide"))?;
            trimmed.split_at(split)
        }
    };
    let color = parse_player(color).map_err(|_| err("couleur attendue : b ou r"))?;
    Ok(ChameleonMove {
        cell: parse_cell(cell, size)?,
        color,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play_all(state: &mut ChameleonState, moves: &[&str]) {
        let n = state.size();
        for m in moves {
            state.play(parse_move(m, n).unwrap()).unwrap();
        }
    }

    #[test]
    fn a_red_chain_west_east_wins_for_blue() {
        // Rouge (Vert) aide involontairement Horz : une chaîne de pierres
        // rouges d'Ouest en Est fait gagner Bleu, quel que soit l'auteur.
        let mut s = ChameleonState::new(3).unwrap();
        play_all(&mut s, &["a2r", "a1b", "b2r", "c1b"]);
        assert_eq!(s.outcome(), None);
        play_all(&mut s, &["c2r"]);
        assert_eq!(s.outcome(), Some(Outcome::Win(BLUE)));
        assert!(s.play(parse_move("a3b", 3).unwrap()).is_err());
    }

    #[test]
    fn a_blue_chain_north_south_wins_for_red() {
        let mut s = ChameleonState::new(3).unwrap();
        play_all(&mut s, &["b1b", "a1r", "b2b", "c3r"]);
        play_all(&mut s, &["b3b"]);
        assert_eq!(s.outcome(), Some(Outcome::Win(RED)));
    }

    #[test]
    fn both_connections_at_once_mover_wins() {
        // Sur 2 × 2, a2 touche l'Ouest et le Sud, b1 le Nord et l'Est, et les
        // deux cases sont voisines : une chaîne a2-b1 relie les deux
        // directions d'un coup. C'est l'auteur du second coup qui gagne.
        for (first, colors) in [(BLUE, ["a2r", "b1r"]), (RED, ["a2b", "b1b"])] {
            let mut s = ChameleonState::with_first_player(2, first).unwrap();
            play_all(&mut s, &colors[..1]);
            let mover = s.current_player();
            play_all(&mut s, &colors[1..]);
            assert_eq!(s.outcome(), Some(Outcome::Win(mover)), "premier {first:?}");
        }
    }

    #[test]
    fn notation_and_positions() {
        for text in ["c4b", "c4r", "C4:Blue", "c4:rouge"] {
            let mv = parse_move(text, 5).unwrap();
            assert_eq!(mv.cell, parse_cell("c4", 5).unwrap());
        }
        assert_eq!(parse_move("c4r", 5).unwrap().color, RED);
        assert_eq!(format_move(parse_move("e5:b", 5).unwrap(), 5), "e5b");
        for bad in ["", "c4", "c4x", "z9b", "c4:vert"] {
            assert!(parse_move(bad, 5).is_err(), "{bad}");
        }
        let mut s = ChameleonState::new(4).unwrap();
        play_all(&mut s, &["b2r", "c3r", "a1b"]);
        let back = ChameleonState::from_position(&s.to_position()).unwrap();
        assert_eq!(back.to_position(), s.to_position());
        assert_eq!(back.ply(), 3);
        assert_eq!(back.current_player(), RED);
        assert_eq!(back.first_player(), BLUE);
    }

    #[test]
    fn swap_transposes_and_keeps_the_color() {
        let mut s = ChameleonState::new(5).unwrap();
        play_all(&mut s, &["b1r"]);
        s.apply_swap().unwrap();
        assert_eq!(s.stone_at(parse_cell("a2", 5).unwrap()), Some(RED));
        assert_eq!(s.stone_at(parse_cell("b1", 5).unwrap()), None);
        assert_eq!((s.current_player(), s.ply()), (BLUE, 2));
        assert!(s.apply_swap().is_err());
    }
}
