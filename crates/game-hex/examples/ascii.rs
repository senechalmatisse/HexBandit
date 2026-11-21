//! Exemple : affiche une position de Hex en ASCII.
use game_hex::{HexState, render_ascii};
fn main() {
    let s = HexState::from_position(".r..../..r.../...r../....r./bbbbbb/r..... r").unwrap();
    print!("{}", render_ascii(&s));
    let t = HexState::new(11).unwrap();
    print!("{}", render_ascii(&t));
}
