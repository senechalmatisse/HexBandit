//! Quarto : variante qui démontre la généricité du moteur.
//!
//! Seize pièces portent chacune quatre caractères binaires (bits 0 à 3 :
//! grande/petite, foncée/claire, carrée/ronde, creuse/pleine). Le plateau fait
//! 4 × 4. À son tour, un joueur pose la pièce que l'adversaire lui a donnée,
//! puis choisit la pièce que l'adversaire devra poser : un coup est donc
//! **composé** (case, pièce offerte). Le premier coup de la partie consiste
//! seulement à offrir une pièce. Celui qui complète une ligne, une colonne ou
//! une diagonale de quatre pièces partageant au moins un caractère gagne
//! (variante avancée : un carré 2 × 2 compte aussi). Plateau plein sans alignement : match nul.
//!
//! Les pièces n'appartiennent à personne et un coup n'a pas de sens hors de
//! son contexte (la pièce posée dépend de l'adversaire) : `QuartoState`
//! n'implémente pas `AmafGame`, si bien que RAVE est refusé par le
//! compilateur pour ce jeu (ségrégation des interfaces). Le crate `mcts`
//! n'a pas été modifié pour accueillir ce jeu.
#![forbid(unsafe_code)]

use game_core::{Game, GameError, Outcome, Player, rng::Rng};

/// Case vide.
const EMPTY: u8 = u8::MAX;
/// Toutes les pièces disponibles.
const ALL_PIECES: u16 = 0xFFFF;

/// Lignes, colonnes et diagonales.
const LINES: [[u8; 4]; 10] = [
    [0, 1, 2, 3],
    [4, 5, 6, 7],
    [8, 9, 10, 11],
    [12, 13, 14, 15],
    [0, 4, 8, 12],
    [1, 5, 9, 13],
    [2, 6, 10, 14],
    [3, 7, 11, 15],
    [0, 5, 10, 15],
    [3, 6, 9, 12],
];

/// Carrés 2 × 2 (variante avancée).
const SQUARES: [[u8; 4]; 9] = [
    [0, 1, 4, 5],
    [1, 2, 5, 6],
    [2, 3, 6, 7],
    [4, 5, 8, 9],
    [5, 6, 9, 10],
    [6, 7, 10, 11],
    [8, 9, 12, 13],
    [9, 10, 13, 14],
    [10, 11, 14, 15],
];

/// Coup composé : case où poser la pièce reçue, pièce offerte à l'adversaire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QuartoMove {
    /// Case (0 à 15, ligne × 4 + colonne) ; `None` au tout premier coup.
    pub cell: Option<u8>,
    /// Pièce offerte (0 à 15) ; `None` si la pose gagne ou s'il n'en reste plus.
    pub give: Option<u8>,
}

/// Vrai si quatre pièces partagent au moins un caractère.
#[inline]
#[must_use]
pub fn share_attribute(pieces: [u8; 4]) -> bool {
    let all = pieces[0] & pieces[1] & pieces[2] & pieces[3];
    let none = !pieces[0] & !pieces[1] & !pieces[2] & !pieces[3];
    (all | none) & 0x0F != 0
}

/// Position de Quarto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuartoState {
    board: [u8; 16],
    available: u16,
    in_hand: Option<u8>,
    to_move: Player,
    first_player: Player,
    ply: u32,
    outcome: Option<Outcome>,
    squares: bool,
}

impl QuartoState {
    /// Partie vide ; `squares` active la variante avancée (carrés 2 × 2).
    #[must_use]
    pub fn new(first_player: Player, squares: bool) -> Self {
        QuartoState {
            board: [EMPTY; 16],
            available: ALL_PIECES,
            in_hand: None,
            to_move: first_player,
            first_player,
            ply: 0,
            outcome: None,
            squares,
        }
    }

    /// Pièce en case `cell`.
    #[must_use]
    pub fn piece_at(&self, cell: usize) -> Option<u8> {
        self.board.get(cell).copied().filter(|&p| p != EMPTY)
    }

    /// Pièce que le joueur au trait doit poser.
    #[must_use]
    pub fn in_hand(&self) -> Option<u8> {
        self.in_hand
    }

    /// Joueur qui a commencé.
    #[must_use]
    pub fn first_player(&self) -> Player {
        self.first_player
    }

    /// Vrai si la variante « carrés » est active.
    #[must_use]
    pub fn squares(&self) -> bool {
        self.squares
    }

    fn patterns(&self) -> impl Iterator<Item = &[u8; 4]> {
        let squares: &[[u8; 4]] = if self.squares { &SQUARES } else { &[] };
        LINES.iter().chain(squares.iter())
    }

    /// Vrai si poser `piece` en `cell` complète un alignement gagnant.
    fn would_win(&self, cell: u8, piece: u8) -> bool {
        self.patterns()
            .filter(|p| p.contains(&cell))
            .any(|pattern| {
                let mut pieces = [0u8; 4];
                for (slot, &c) in pieces.iter_mut().zip(pattern) {
                    let p = if c == cell {
                        piece
                    } else {
                        self.board[usize::from(c)]
                    };
                    if p == EMPTY {
                        return false;
                    }
                    *slot = p;
                }
                share_attribute(pieces)
            })
    }

    fn illegal(mv: QuartoMove, reason: &str) -> GameError {
        GameError::IllegalMove {
            mv: format_move(mv),
            reason: reason.to_owned(),
        }
    }
}

impl Game for QuartoState {
    type Move = QuartoMove;

    fn current_player(&self) -> Player {
        self.to_move
    }

    fn legal_moves(&self, out: &mut Vec<QuartoMove>) {
        out.clear();
        if self.outcome.is_some() {
            return;
        }
        let gifts = (0..16u8).filter(|&p| self.available & (1 << p) != 0);
        match self.in_hand {
            None => out.extend(gifts.map(|p| QuartoMove {
                cell: None,
                give: Some(p),
            })),
            Some(piece) => {
                for cell in (0..16u8).filter(|&c| self.board[usize::from(c)] == EMPTY) {
                    if self.would_win(cell, piece) || self.available == 0 {
                        out.push(QuartoMove {
                            cell: Some(cell),
                            give: None,
                        });
                    } else {
                        out.extend(gifts.clone().map(|p| QuartoMove {
                            cell: Some(cell),
                            give: Some(p),
                        }));
                    }
                }
            }
        }
    }

    fn play(&mut self, mv: QuartoMove) -> Result<(), GameError> {
        if self.outcome.is_some() {
            return Err(GameError::GameOver);
        }
        let mut wins = false;
        match (self.in_hand, mv.cell) {
            (None, Some(_)) => {
                return Err(Self::illegal(mv, "aucune pièce à poser au premier coup"));
            }
            (Some(_), None) => return Err(Self::illegal(mv, "il faut poser la pièce reçue")),
            (None, None) => {}
            (Some(piece), Some(cell)) => {
                if cell >= 16 {
                    return Err(GameError::OutOfBounds {
                        mv: format_move(mv),
                    });
                }
                if self.board[usize::from(cell)] != EMPTY {
                    return Err(GameError::Occupied {
                        mv: format_move(mv),
                    });
                }
                wins = self.would_win(cell, piece);
            }
        }
        let must_give = !wins && self.available != 0;
        match (must_give, mv.give) {
            (true, Some(p)) if p < 16 && self.available & (1 << p) != 0 => {}
            (true, _) => return Err(Self::illegal(mv, "pièce offerte indisponible ou manquante")),
            (false, Some(_)) => return Err(Self::illegal(mv, "aucune pièce à offrir")),
            (false, None) => {}
        }
        // Le coup est légal : on l'applique.
        if let (Some(piece), Some(cell)) = (self.in_hand, mv.cell) {
            self.board[usize::from(cell)] = piece;
        }
        if wins {
            self.outcome = Some(Outcome::Win(self.to_move));
            self.in_hand = None;
        } else if self.board.iter().all(|&p| p != EMPTY) {
            self.outcome = Some(Outcome::Draw);
            self.in_hand = None;
        } else {
            let give = mv.give.unwrap_or(0);
            self.in_hand = Some(give);
            self.available &= !(1 << give);
        }
        self.to_move = self.to_move.opponent();
        self.ply += 1;
        Ok(())
    }

    fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    fn ply(&self) -> u32 {
        self.ply
    }

    fn random_legal_move<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        buf: &mut Vec<QuartoMove>,
    ) -> Option<QuartoMove> {
        if self.outcome.is_some() {
            return None;
        }
        // Prendre un coup gagnant s'il existe n'est pas fait ici : la politique
        // reste uniforme, comme la politique par défaut de l'article.
        self.legal_moves(buf);
        if buf.is_empty() {
            None
        } else {
            Some(buf[rng.below(buf.len() as u32) as usize])
        }
    }
}

/// Notation d'un coup : `c3/7` (poser en c3, offrir la pièce 7), `c3` (poser
/// sans offrir), `/7` (premier coup : offrir la pièce 7). Pièces en hexadécimal.
#[must_use]
pub fn format_move(mv: QuartoMove) -> String {
    let cell = mv
        .cell
        .map(|c| format!("{}{}", char::from(b'a' + c % 4), c / 4 + 1))
        .unwrap_or_default();
    match mv.give {
        Some(p) => format!("{cell}/{p:x}"),
        None => cell,
    }
}

/// Lecture de la notation de [`format_move`].
///
/// # Errors
///
/// [`GameError::Parse`] si le texte est mal formé.
pub fn parse_move(text: &str) -> Result<QuartoMove, GameError> {
    let err = |reason: &str| GameError::Parse {
        input: text.to_owned(),
        reason: reason.to_owned(),
    };
    let trimmed = text.trim().to_ascii_lowercase();
    let (cell_part, give_part) = match trimmed.split_once('/') {
        Some((c, g)) => (c, Some(g)),
        None => (trimmed.as_str(), None),
    };
    let cell = if cell_part.is_empty() {
        None
    } else {
        let bytes = cell_part.as_bytes();
        if bytes.len() != 2
            || !(b'a'..=b'd').contains(&bytes[0])
            || !(b'1'..=b'4').contains(&bytes[1])
        {
            return Err(err("case attendue entre a1 et d4"));
        }
        Some((bytes[1] - b'1') * 4 + (bytes[0] - b'a'))
    };
    let give = match give_part {
        None => None,
        Some(g) => Some(
            u8::from_str_radix(g, 16)
                .ok()
                .filter(|&p| p < 16)
                .ok_or_else(|| err("pièce attendue entre 0 et f"))?,
        ),
    };
    if cell.is_none() && give.is_none() {
        return Err(err("coup vide"));
    }
    Ok(QuartoMove { cell, give })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_sharing() {
        assert!(share_attribute([0b0001, 0b0011, 0b0101, 0b1001]));
        assert!(share_attribute([0b1110, 0b1100, 0b1010, 0b0110])); // bit 0 nul partout
        assert!(!share_attribute([0b0000, 0b1111, 0b0101, 0b1010]));
    }

    #[test]
    fn notation_round_trip() {
        for mv in [
            QuartoMove {
                cell: None,
                give: Some(7),
            },
            QuartoMove {
                cell: Some(10),
                give: Some(15),
            },
            QuartoMove {
                cell: Some(15),
                give: None,
            },
        ] {
            assert_eq!(parse_move(&format_move(mv)).unwrap(), mv);
        }
        assert_eq!(
            format_move(QuartoMove {
                cell: Some(10),
                give: Some(11)
            }),
            "c3/b"
        );
        for bad in ["", "e1/3", "a5", "a1/g", "/"] {
            assert!(parse_move(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn winning_row_and_square() {
        // Pièces 1, 3, 5, 9 (bit 0 commun) sur la première ligne.
        let mut s = QuartoState::new(Player::P1, false);
        let moves = ["/1", "a1/3", "b1/5", "c1/9", "d1"];
        for m in moves {
            s.play(parse_move(m).unwrap()).unwrap();
        }
        assert_eq!(s.outcome(), Some(Outcome::Win(Player::P1)));
        // Variante avancée : carré a1 b1 a2 b2.
        let mut s = QuartoState::new(Player::P1, true);
        for m in ["/1", "a1/3", "b1/5", "a2/9", "b2"] {
            s.play(parse_move(m).unwrap()).unwrap();
        }
        assert_eq!(s.outcome(), Some(Outcome::Win(Player::P1)));
        // Sans la variante, le même carré ne gagne pas : il faut offrir une pièce.
        let mut s = QuartoState::new(Player::P1, false);
        for m in ["/1", "a1/3", "b1/5", "a2/9"] {
            s.play(parse_move(m).unwrap()).unwrap();
        }
        assert!(s.play(parse_move("b2").unwrap()).is_err());
        assert!(s.play(parse_move("b2/0").unwrap()).is_ok());
    }

    #[test]
    fn illegal_moves_are_typed() {
        let mut s = QuartoState::new(Player::P1, false);
        assert!(s.play(parse_move("a1/3").unwrap()).is_err());
        s.play(parse_move("/3").unwrap()).unwrap();
        assert!(s.play(parse_move("/4").unwrap()).is_err());
        assert!(
            s.play(parse_move("a1/3").unwrap()).is_err(),
            "pièce 3 déjà donnée"
        );
        s.play(parse_move("a1/4").unwrap()).unwrap();
        assert!(matches!(
            s.play(parse_move("a1/5").unwrap()),
            Err(GameError::Occupied { .. })
        ));
    }
}
