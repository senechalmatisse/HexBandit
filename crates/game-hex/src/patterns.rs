//! Motifs locaux du Hex : connaissance a priori pour MCTS, sans dépendre du moteur
//! (l'adaptateur `PriorKnowledge` est dans le crate `bindings`).
//!
//! Les six voisins d'une case forment un anneau ; deux voisins consécutifs de
//! l'anneau sont voisins entre eux. Les cases hors du plateau sont remplacées
//! par le bord correspondant, qui appartient au joueur qui doit le relier
//! (Ouest et Est à Bleu, Nord et Sud à Rouge).
//!
//! * **Case morte** : case vide dont la couleur ne peut plus changer le
//!   vainqueur. Critère suffisant (sûr) : pour toute couleur future de ses
//!   voisins vides, les voisins de chaque couleur forment un seul arc de
//!   l'anneau ; ils sont alors déjà reliés entre eux sans elle, et la case ne
//!   relie rien de nouveau, quelle que soit sa couleur. Jouer une case morte
//!   revient à passer, ce qui n'est jamais avantageux au Hex.
//! * **Pont** : deux pierres (ou une pierre et son bord) ayant deux voisins
//!   communs vides ; si l'adversaire en occupe un, jouer l'autre **sauve** le
//!   pont, et pour l'adversaire, l'occuper le **coupe**.
//! * **Gabarit de bord** (pont vers le bord) : case de la deuxième rangée
//!   dont les deux voisins de la première rangée sont vides.

use game_core::{Game, Player};

use crate::{BLUE, Cell, HexState, RED};

/// Contenu d'un voisin de l'anneau.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    /// Case vide.
    Empty,
    /// Pierre ou bord appartenant à ce joueur.
    Own(Player),
    /// Hors du plateau, au coin de deux bords de joueurs différents : traité
    /// comme une case de couleur inconnue (prudent).
    Corner,
}

/// Décalages (ligne, colonne) des six voisins, dans l'ordre de l'anneau.
const RING: [(isize, isize); 6] = [(-1, 0), (-1, 1), (0, 1), (1, 0), (1, -1), (0, -1)];

/// Anneau des six voisins de `cell`.
fn ring(state: &HexState, cell: Cell) -> [Slot; 6] {
    let n = state.size() as isize;
    let (r, c) = cell.rc(state.size());
    let (r, c) = (r as isize, c as isize);
    let mut out = [Slot::Empty; 6];
    for (slot, (dr, dc)) in out.iter_mut().zip(RING) {
        let (nr, nc) = (r + dr, c + dc);
        let off_ns = nr < 0 || nr >= n;
        let off_we = nc < 0 || nc >= n;
        *slot = match (off_ns, off_we) {
            (false, false) => match state.stone_at_rc(nr as usize, nc as usize) {
                None => Slot::Empty,
                Some(p) => Slot::Own(p),
            },
            (true, false) => Slot::Own(RED),
            (false, true) => Slot::Own(BLUE),
            (true, true) => Slot::Corner,
        };
    }
    out
}

/// Vrai si, dans un anneau sans case vide, les cases de chaque joueur
/// forment au plus un arc.
fn arcs_are_contiguous(colors: [Player; 6]) -> bool {
    // Nombre de changements de couleur le long de l'anneau : 0 (une seule
    // couleur) ou 2 (deux arcs) ; 4 ou plus, une couleur est coupée en deux.
    (0..6).filter(|&i| colors[i] != colors[(i + 1) % 6]).count() <= 2
}

/// Vrai si la case vide `cell` est morte (critère local, voir le module).
/// Une case occupée n'est pas morte.
#[must_use]
pub fn is_dead(state: &HexState, cell: Cell) -> bool {
    if state.stone_at(cell).is_some() {
        return false;
    }
    let slots = ring(state, cell);
    let unknown: Vec<usize> = (0..6)
        .filter(|&i| matches!(slots[i], Slot::Empty | Slot::Corner))
        .collect();
    if unknown.len() > 4 {
        // Trop de voisins libres : une coloration alternée existe toujours.
        return false;
    }
    (0u32..1 << unknown.len()).all(|mask| {
        let mut colors = [BLUE; 6];
        for (i, slot) in slots.iter().enumerate() {
            if let Slot::Own(p) = slot {
                colors[i] = *p;
            }
        }
        for (bit, &i) in unknown.iter().enumerate() {
            colors[i] = if mask >> bit & 1 == 1 { RED } else { BLUE };
        }
        arcs_are_contiguous(colors)
    })
}

/// Nombre de ponts de `player` que jouer en `cell` (vide) sauve : deux
/// voisins de l'anneau à distance 2 (pierres ou bord de `player`) dont le
/// voisin intermédiaire est occupé par l'adversaire.
#[must_use]
pub fn bridge_saves(state: &HexState, cell: Cell, player: Player) -> u32 {
    if state.stone_at(cell).is_some() {
        return 0;
    }
    let slots = ring(state, cell);
    let opponent = player.opponent();
    (0..6)
        .filter(|&i| {
            slots[i] == Slot::Own(player)
                && slots[(i + 2) % 6] == Slot::Own(player)
                && slots[(i + 1) % 6] == Slot::Own(opponent)
        })
        .count() as u32
}

/// Nombre de nouveaux ponts que crée une pierre de `player` en `cell` (vide) :
/// pierres de `player` à distance de pont dont les deux voisins communs sont vides.
#[must_use]
pub fn new_bridges(state: &HexState, cell: Cell, player: Player) -> u32 {
    if state.stone_at(cell).is_some() {
        return 0;
    }
    let n = state.size() as isize;
    let (r, c) = cell.rc(state.size());
    let (r, c) = (r as isize, c as isize);
    let inside = |rr: isize, cc: isize| rr >= 0 && cc >= 0 && rr < n && cc < n;
    let empty = |rr: isize, cc: isize| {
        inside(rr, cc) && state.stone_at_rc(rr as usize, cc as usize).is_none()
    };
    let mut count = 0;
    for k in 0..6 {
        let (a, b) = (RING[k], RING[(k + 1) % 6]);
        let (br, bc) = (r + a.0 + b.0, c + a.1 + b.1);
        if inside(br, bc)
            && state.stone_at_rc(br as usize, bc as usize) == Some(player)
            && empty(r + a.0, c + a.1)
            && empty(r + b.0, c + b.1)
        {
            count += 1;
        }
    }
    count
}

/// Vrai si `cell` (vide) est sur la deuxième rangée d'un bord de `player` et
/// que ses deux voisins de la première rangée sont vides (pont vers le bord).
#[must_use]
pub fn edge_template(state: &HexState, cell: Cell, player: Player) -> bool {
    if state.stone_at(cell).is_some() {
        return false;
    }
    let n = state.size();
    if n < 3 {
        return false;
    }
    let (r, c) = cell.rc(n);
    let empty = |rr: usize, cc: usize| state.stone_at_rc(rr, cc).is_none();
    match player {
        Player::P1 => {
            (c == 1 && r + 1 < n && empty(r, 0) && empty(r + 1, 0))
                || (c == n - 2 && r >= 1 && empty(r, n - 1) && empty(r - 1, n - 1))
        }
        Player::P2 => {
            (r == 1 && c + 1 < n && empty(0, c) && empty(0, c + 1))
                || (r == n - 2 && c >= 1 && empty(n - 1, c) && empty(n - 1, c - 1))
        }
    }
}

/// Caractéristiques d'un coup pour le joueur au trait.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MoveFeatures {
    /// Case morte.
    pub dead: bool,
    /// Ponts du joueur sauvés par le coup.
    pub saves: u32,
    /// Ponts adverses coupés par le coup (l'adversaire y aurait sauvé un pont).
    pub cuts: u32,
    /// Nouveaux ponts créés.
    pub bridges: u32,
    /// Pont vers un bord du joueur.
    pub edge_template: bool,
}

/// Caractéristiques du coup `cell` pour le joueur au trait de `state`.
#[must_use]
pub fn move_features(state: &HexState, cell: Cell) -> MoveFeatures {
    let player = state.current_player();
    if is_dead(state, cell) {
        return MoveFeatures {
            dead: true,
            ..MoveFeatures::default()
        };
    }
    MoveFeatures {
        dead: false,
        saves: bridge_saves(state, cell, player),
        cuts: bridge_saves(state, cell, player.opponent()),
        bridges: new_bridges(state, cell, player),
        edge_template: edge_template(state, cell, player),
    }
}

#[cfg(test)]
mod tests {
    use game_core::rng::{Pcg32, Rng};

    use super::*;
    use crate::notation::parse_cell;

    fn position(n: usize, moves: &[&str]) -> HexState {
        let cells: Vec<Cell> = moves.iter().map(|m| parse_cell(m, n).unwrap()).collect();
        HexState::from_moves(n, BLUE, &cells).unwrap()
    }

    /// Vérité terrain : la couleur de `cell` ne change le vainqueur d'aucun
    /// remplissage du plateau (tous les remplissages des cases vides restantes).
    fn dead_by_brute_force(state: &HexState, cell: Cell) -> bool {
        let n = state.size();
        let empties: Vec<usize> = (0..n * n)
            .filter(|&i| i != usize::from(cell.0) && state.stone_at(Cell(i as u16)).is_none())
            .collect();
        assert!(empties.len() <= 14, "trop de cases pour la force brute");
        let base: Vec<Option<Player>> =
            (0..n * n).map(|i| state.stone_at(Cell(i as u16))).collect();
        (0u32..1 << empties.len()).all(|mask| {
            let mut board = base.clone();
            for (bit, &i) in empties.iter().enumerate() {
                board[i] = Some(if mask >> bit & 1 == 1 { RED } else { BLUE });
            }
            let winner = |color: Player| {
                let mut b = board.clone();
                b[usize::from(cell.0)] = Some(color);
                blue_connects(n, &b)
            };
            winner(BLUE) == winner(RED)
        })
    }

    fn blue_connects(n: usize, board: &[Option<Player>]) -> bool {
        let mut seen = vec![false; n * n];
        let mut stack: Vec<usize> = (0..n)
            .map(|r| r * n)
            .filter(|&i| board[i] == Some(BLUE))
            .collect();
        for &i in &stack {
            seen[i] = true;
        }
        while let Some(i) = stack.pop() {
            let (r, c) = ((i / n) as isize, (i % n) as isize);
            if c as usize == n - 1 {
                return true;
            }
            for (dr, dc) in RING {
                let (nr, nc) = (r + dr, c + dc);
                if nr >= 0 && nc >= 0 && (nr as usize) < n && (nc as usize) < n {
                    let j = nr as usize * n + nc as usize;
                    if !seen[j] && board[j] == Some(BLUE) {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        false
    }

    #[test]
    fn dead_cells_are_really_dead() {
        // Positions aléatoires de 4 × 4 : chaque case déclarée morte l'est
        // (sûreté), et le critère en trouve.
        let mut rng = Pcg32::new(17, 3);
        let mut found = 0;
        for _ in 0..400 {
            let mut state = HexState::new(4).unwrap();
            let plies = 6 + rng.below(6) as usize;
            let mut buf = Vec::new();
            for _ in 0..plies {
                let Some(mv) = state.random_legal_move(&mut rng, &mut buf) else {
                    break;
                };
                state.play(mv).unwrap();
            }
            if state.is_terminal() {
                continue;
            }
            for i in 0..16u16 {
                let cell = Cell(i);
                if is_dead(&state, cell) {
                    found += 1;
                    assert!(
                        dead_by_brute_force(&state, cell),
                        "{} : {i}",
                        state.to_position()
                    );
                }
            }
        }
        assert!(found > 20, "{found} cases mortes trouvées");
    }

    #[test]
    fn classic_dead_cell_and_bridges() {
        // c3 : quatre voisins bleus consécutifs (c2, d2, d3, c4) et deux
        // rouges (b4, b3) : morte, quelle que soit la suite.
        let s = HexState::from_position("r..../..bb./.r.b./.rb../....r b").unwrap();
        assert!(is_dead(&s, parse_cell("c3", 5).unwrap()));
        assert!(move_features(&s, parse_cell("c3", 5).unwrap()).dead);
        assert!(!is_dead(&s, parse_cell("e1", 5).unwrap()));
        let s = HexState::from_position("...../.b.../...../...../..... r").unwrap();
        assert!(!is_dead(&s, parse_cell("a1", 5).unwrap()));
        // Pont b2-c3 (cases communes c2 et b3) : Bleu crée un pont en jouant c3.
        assert_eq!(new_bridges(&s, parse_cell("c3", 5).unwrap(), BLUE), 1);
        // Rouge s'immisce en c2 : Bleu sauve en b3.
        let s = position(5, &["b2", "a5", "c3", "c2"]);
        assert_eq!(bridge_saves(&s, parse_cell("b3", 5).unwrap(), BLUE), 1);
        // Pour Rouge, b3 coupe ce pont.
        assert_eq!(bridge_saves(&s, parse_cell("b3", 5).unwrap(), RED), 0);
        let f = move_features(&s, parse_cell("b3", 5).unwrap());
        assert_eq!((f.saves, f.dead), (1, false));
        // Gabarit de bord : b3 (colonne 1) avec a3 et a4 vides, pour Bleu.
        let s = HexState::new(5).unwrap();
        assert!(edge_template(&s, parse_cell("b3", 5).unwrap(), BLUE));
        assert!(!edge_template(&s, parse_cell("b3", 5).unwrap(), RED));
        assert!(edge_template(&s, parse_cell("c2", 5).unwrap(), RED));
        assert!(edge_template(&s, parse_cell("d3", 5).unwrap(), BLUE));
    }
}
