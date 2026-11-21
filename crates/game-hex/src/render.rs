//! Rendu ASCII du plateau. Renvoie une chaîne : aucune entrée-sortie.

use game_core::{Game, Player};

use crate::HexState;

/// Dessine le plateau en losange. Chaque ligne est décalée d'un espace vers
/// la droite ; `B` = Bleu, `R` = Rouge, `.` = vide, `*` = pierre du chemin gagnant.
///
/// ```
/// use game_hex::{HexState, render_ascii};
/// let board = render_ascii(&HexState::new(3).unwrap());
/// assert!(board.contains("a b c"));
/// ```
#[must_use]
pub fn render_ascii(state: &HexState) -> String {
    let n = state.size();
    let path = state.winning_path().unwrap_or_default();
    let letters: Vec<String> = (0..n)
        .map(|c| char::from(b'a' + c as u8).to_string())
        .collect();
    let header = letters.join(" ");
    let mut out = String::new();
    out.push_str(&format!("    {header}\n"));
    for row in 0..n {
        let indent = " ".repeat(row);
        let mut line = format!("{indent}{:>2}  ", row + 1);
        for col in 0..n {
            let cell = crate::Cell((row * n + col) as u16);
            let ch = match state.stone_at_rc(row, col) {
                None => '.',
                Some(_) if path.contains(&cell) => '*',
                Some(Player::P1) => 'B',
                Some(Player::P2) => 'R',
            };
            line.push(ch);
            if col + 1 < n {
                line.push(' ');
            }
        }
        line.push_str(&format!("  {}\n", row + 1));
        out.push_str(&line);
    }
    out.push_str(&format!("{}{header}\n", " ".repeat(n + 3)));
    let status = match state.outcome() {
        Some(o) => match o.winner() {
            Some(Player::P1) => "Victoire de Bleu".to_owned(),
            Some(Player::P2) => "Victoire de Rouge".to_owned(),
            None => "Nul".to_owned(),
        },
        None => format!(
            "Trait : {}",
            if state.current_player() == Player::P1 {
                "Bleu"
            } else {
                "Rouge"
            }
        ),
    };
    out.push_str(&format!(
        "Bleu (B) : Ouest-Est | Rouge (R) : Nord-Sud | {status}\n"
    ));
    out
}
