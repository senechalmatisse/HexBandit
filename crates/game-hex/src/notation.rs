//! Notation des coups et des positions.
//!
//! * Coup : colonne en lettre (`a` = colonne 0), ligne en chiffre (`1` =
//!   ligne 0). Exemple : `c4` désigne la ligne 3, colonne 2.
//! * Position : lignes du haut vers le bas séparées par `/`, une case par
//!   caractère (`.` vide, `b` Bleu, `r` Rouge), puis un espace et le joueur au
//!   trait (`b` ou `r`). Exemple : `.b./.../r.. b`.

use game_core::{Game, GameError, Player};

use crate::{BLUE, Cell, HexState, MAX_SIZE, RED, state::EMPTY, state::owner, state::stone};

/// Écrit un coup en notation `c4`.
#[must_use]
pub fn format_cell(cell: Cell, size: usize) -> String {
    let index = usize::from(cell.0);
    if size == 0 || index >= size * size {
        return format!("#{index}");
    }
    let (row, col) = (index / size, index % size);
    format!("{}{}", char::from(b'a' + col as u8), row + 1)
}

/// Lit un coup en notation `c4` (insensible à la casse).
///
/// # Errors
///
/// [`GameError::Parse`] si le texte est mal formé ou hors du plateau.
pub fn parse_cell(text: &str, size: usize) -> Result<Cell, GameError> {
    let err = |reason: &str| GameError::Parse {
        input: text.to_owned(),
        reason: reason.to_owned(),
    };
    let trimmed = text.trim();
    let mut chars = trimmed.chars();
    let letter = chars
        .next()
        .ok_or_else(|| err("coup vide"))?
        .to_ascii_lowercase();
    if !letter.is_ascii_lowercase() {
        return Err(err("la colonne doit être une lettre"));
    }
    let col = (letter as u8 - b'a') as usize;
    let digits: &str = chars.as_str();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(err("la ligne doit être un nombre"));
    }
    let row: usize = digits.parse().map_err(|_| err("ligne illisible"))?;
    if row == 0 || row > size || col >= size || size > MAX_SIZE {
        return Err(err("case hors du plateau"));
    }
    Ok(Cell(((row - 1) * size + col) as u16))
}

/// Lettre d'un joueur dans la notation de position.
#[must_use]
pub fn player_letter(player: Player) -> char {
    match player {
        Player::P1 => 'b',
        Player::P2 => 'r',
    }
}

/// Nom d'un joueur (`blue` ou `red`).
#[must_use]
pub fn player_name(player: Player) -> &'static str {
    match player {
        Player::P1 => "blue",
        Player::P2 => "red",
    }
}

/// Lit un nom de joueur (`blue`/`b`/`bleu` ou `red`/`r`/`rouge`).
///
/// # Errors
///
/// [`GameError::Parse`] si le nom est inconnu.
pub fn parse_player(text: &str) -> Result<Player, GameError> {
    match text.trim().to_ascii_lowercase().as_str() {
        "blue" | "b" | "bleu" => Ok(BLUE),
        "red" | "r" | "rouge" => Ok(RED),
        _ => Err(GameError::Parse {
            input: text.to_owned(),
            reason: "joueur inconnu".to_owned(),
        }),
    }
}

impl HexState {
    /// Position au format `lignes/... trait`.
    #[must_use]
    pub fn to_position(&self) -> String {
        let n = self.size();
        let mut out = String::with_capacity(n * (n + 1) + 2);
        for row in 0..n {
            if row > 0 {
                out.push('/');
            }
            for col in 0..n {
                out.push(match self.stone_at_rc(row, col) {
                    None => '.',
                    Some(p) => player_letter(p),
                });
            }
        }
        out.push(' ');
        out.push(player_letter(self.current_player()));
        out
    }

    /// Lit une position au format produit par [`HexState::to_position`].
    ///
    /// Le premier joueur est déduit du nombre de pierres : à égalité, c'est le
    /// joueur au trait ; sinon, celui qui a une pierre de plus.
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
        let to_move = match parts.next() {
            Some(p) => parse_player(p)?,
            None => return Err(err("joueur au trait manquant")),
        };
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
                    '.' => EMPTY,
                    'b' => stone(BLUE),
                    'r' => stone(RED),
                    _ => return Err(err("caractère de case inconnu")),
                });
            }
        }
        let count = |p: Player| cells.iter().filter(|&&c| owner(c) == Some(p)).count();
        let (blue, red) = (count(BLUE), count(RED));
        let first = if blue == red {
            to_move
        } else if blue == red + 1 && to_move == RED {
            BLUE
        } else if red == blue + 1 && to_move == BLUE {
            RED
        } else {
            return Err(err(
                "nombre de pierres incompatible avec le joueur au trait",
            ));
        };
        HexState::from_parts(n, cells, to_move, first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_round_trip() {
        for n in [2usize, 5, 14, 19] {
            for i in 0..(n * n) as u16 {
                let text = format_cell(Cell(i), n);
                assert_eq!(parse_cell(&text, n).unwrap(), Cell(i));
            }
        }
        assert_eq!(format_cell(Cell(3 * 14 + 2), 14), "c4");
        assert_eq!(parse_cell("C4", 14).unwrap(), Cell(3 * 14 + 2));
        for bad in ["", "4", "c", "c0", "c15", "o1", "c4x", "#3"] {
            assert!(parse_cell(bad, 14).is_err(), "{bad}");
        }
    }

    #[test]
    fn position_round_trip() {
        let moves: Vec<Cell> = ["c3", "b2", "a1", "c1"]
            .iter()
            .map(|m| parse_cell(m, 4).unwrap())
            .collect();
        let s = HexState::from_moves(4, RED, &moves).unwrap();
        let text = s.to_position();
        let back = HexState::from_position(&text).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.ply(), 4);
        assert_eq!(back.first_player(), RED);
        assert!(HexState::from_position("bb./.../... r").is_err());
        assert!(HexState::from_position("../.. x").is_err());
        assert!(HexState::from_position("..,/.../... b").is_err());
    }

    #[test]
    fn players() {
        assert_eq!(parse_player("Rouge").unwrap(), RED);
        assert_eq!(player_name(BLUE), "blue");
        assert!(parse_player("vert").is_err());
    }
}
