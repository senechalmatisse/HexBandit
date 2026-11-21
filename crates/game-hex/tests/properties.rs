//! Tests de propriétés, en remplacement de proptest : chaque propriété
//! est vérifiée sur de nombreux cas tirés d'un générateur à graine fixe ; en
//! cas d'échec, la graine et le cas sont affichés pour reproduire.

use game_core::{
    Game, Outcome, Player,
    rng::{Pcg32, Rng, shuffle},
    rollout::{RolloutPolicy, Trace, TraceMode},
};
use game_hex::{BLUE, Cell, HexRollout, HexState, RED, notation::format_cell, render_ascii};

/// Connexion de référence par parcours en profondeur, indépendante de l'union-find.
fn reference_connected(state: &HexState, player: Player) -> bool {
    let n = state.size();
    let owned = |r: usize, c: usize| state.stone_at_rc(r, c) == Some(player);
    let mut seen = vec![false; n * n];
    let mut stack = Vec::new();
    for i in 0..n {
        let (r, c) = if player == BLUE { (i, 0) } else { (0, i) };
        if owned(r, c) {
            seen[r * n + c] = true;
            stack.push((r, c));
        }
    }
    while let Some((r, c)) = stack.pop() {
        if (player == BLUE && c == n - 1) || (player == RED && r == n - 1) {
            return true;
        }
        for (dr, dc) in [(-1isize, 0isize), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0)] {
            let (nr, nc) = (r as isize + dr, c as isize + dc);
            if nr < 0 || nc < 0 || nr >= n as isize || nc >= n as isize {
                continue;
            }
            let (nr, nc) = (nr as usize, nc as usize);
            if !seen[nr * n + nc] && owned(nr, nc) {
                seen[nr * n + nc] = true;
                stack.push((nr, nc));
            }
        }
    }
    false
}

#[test]
fn full_board_has_exactly_one_winner() {
    let mut rng = Pcg32::new(2026, 1);
    for case in 0..2000 {
        let n = 2 + rng.below(18) as usize;
        let mut cells: Vec<u16> = (0..(n * n) as u16).collect();
        shuffle(&mut rng, &mut cells);
        // Plateau plein : on ignore la fin de partie en construisant la position.
        let mut board = vec![b'.'; n * n];
        for (i, &c) in cells.iter().enumerate() {
            board[usize::from(c)] = if i % 2 == 0 { b'b' } else { b'r' };
        }
        let rows: Vec<String> = board
            .chunks(n)
            .map(|row| String::from_utf8(row.to_vec()).unwrap())
            .collect();
        let to_move = if (n * n) % 2 == 0 { "b" } else { "r" };
        let text = format!("{} {to_move}", rows.join("/"));
        let state = HexState::from_position(&text)
            .unwrap_or_else(|e| panic!("cas {case} (n={n}) : {e}\n{text}"));
        let blue = reference_connected(&state, BLUE);
        let red = reference_connected(&state, RED);
        assert!(blue ^ red, "cas {case} : bleu={blue} rouge={red}\n{text}");
        let expected = if blue { BLUE } else { RED };
        assert_eq!(state.winner(), Some(expected), "cas {case}\n{text}");
    }
}

#[test]
fn union_find_matches_reference_dfs_after_every_move() {
    let mut rng = Pcg32::new(7, 7);
    let mut buf = Vec::new();
    for case in 0..1500 {
        let n = 2 + rng.below(13) as usize;
        let first = if rng.below(2) == 0 { BLUE } else { RED };
        let mut state = HexState::with_first_player(n, first).unwrap();
        let mut history = Vec::new();
        while !state.is_terminal() {
            let mv = state.random_legal_move(&mut rng, &mut buf).unwrap();
            state.play(mv).unwrap();
            history.push(format_cell(mv, n));
            let blue = reference_connected(&state, BLUE);
            let red = reference_connected(&state, RED);
            assert!(!(blue && red), "cas {case} : {history:?}");
            let expected = if blue {
                Some(BLUE)
            } else if red {
                Some(RED)
            } else {
                None
            };
            assert_eq!(state.winner(), expected, "cas {case} : {history:?}");
        }
        // Le dernier coup est celui du vainqueur.
        let last_mover = state.current_player().opponent();
        assert_eq!(state.outcome(), Some(Outcome::Win(last_mover)));
    }
}

#[test]
fn reconstruction_from_moves_and_position_is_identical() {
    let mut rng = Pcg32::new(11, 3);
    let mut buf = Vec::new();
    for _ in 0..300 {
        let n = 2 + rng.below(18) as usize;
        let mut state = HexState::new(n).unwrap();
        let mut moves: Vec<Cell> = Vec::new();
        let plies = rng.below((n * n) as u32);
        for _ in 0..plies {
            match state.random_legal_move(&mut rng, &mut buf) {
                Some(mv) => {
                    state.play(mv).unwrap();
                    moves.push(mv);
                }
                None => break,
            }
        }
        let rebuilt = HexState::from_moves(n, BLUE, &moves).unwrap();
        assert_eq!(rebuilt, state);
        let parsed = HexState::from_position(&state.to_position()).unwrap();
        assert_eq!(parsed, state);
        assert_eq!(parsed.ply(), state.ply());
        assert!(render_ascii(&state).lines().count() >= n + 3);
    }
}

/// Le remplissage aléatoire et les coups uniformes un par un doivent donner
/// la même loi du vainqueur.
#[test]
fn fill_and_uniform_rollouts_have_the_same_winner_distribution() {
    let start = HexState::from_moves(6, BLUE, &[Cell(14), Cell(3)]).unwrap();
    let mut rng = Pcg32::new(5, 5);
    let mut trace = Trace::new(TraceMode::Off);
    let trials = 40_000;
    let mut wins = [0u32; 2];
    for (k, policy) in [HexRollout::fill(), HexRollout::uniform()]
        .iter_mut()
        .enumerate()
    {
        for _ in 0..trials {
            let mut s = start.clone();
            if policy.rollout(&mut s, &mut rng, &mut trace) == Outcome::Win(BLUE) {
                wins[k] += 1;
            }
        }
    }
    let p = [
        f64::from(wins[0]) / f64::from(trials),
        f64::from(wins[1]) / f64::from(trials),
    ];
    let se = ((p[0] * (1.0 - p[0]) + p[1] * (1.0 - p[1])) / f64::from(trials)).sqrt();
    assert!((p[0] - p[1]).abs() < 4.0 * se, "remplissage {p:?}");
}

#[test]
fn ascii_rendering_marks_winning_path() {
    let moves: Vec<Cell> = [0u16, 6, 1, 7, 2].iter().map(|&c| Cell(c)).collect();
    let s = HexState::from_moves(3, BLUE, &moves).unwrap();
    let text = render_ascii(&s);
    assert!(text.contains("* * *"), "{text}");
    assert!(text.contains("Victoire de Bleu"), "{text}");
}
