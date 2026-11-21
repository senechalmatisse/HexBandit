//! Tests de contrat et propriétés de Chameleon.

use game_chameleon::{ChameleonMove, ChameleonState};
use game_core::{
    AmafGame, Game, Outcome,
    contract::{check_amaf_contract, check_game_contract, check_rollout_contract},
    decorators::{SwapRollout, SwapRule},
    rng::Pcg32,
    rollout::{RolloutPolicy, Trace, TraceMode, UniformRollout},
};
use game_hex::{BLUE, RED};

#[test]
fn chameleon_satisfies_the_contracts() {
    for size in [2usize, 3, 5, 9] {
        let initial = ChameleonState::new(size).unwrap();
        check_game_contract(&initial, size as u64, 40, 2 * size * size).unwrap();
        check_amaf_contract(&initial, 10 + size as u64, 20).unwrap();
        check_rollout_contract(&initial, &mut UniformRollout::new(), 20 + size as u64, 60).unwrap();
        let swap = SwapRule::new(ChameleonState::new(size).unwrap());
        check_game_contract(&swap, 30 + size as u64, 20, 2 * size * size).unwrap();
        check_amaf_contract(&swap, 40 + size as u64, 10).unwrap();
        let mut policy = SwapRollout::new(UniformRollout::<ChameleonState>::new());
        check_rollout_contract(&swap, &mut policy, 50 + size as u64, 30).unwrap();
    }
}

#[test]
fn amaf_index_separates_the_two_colors_of_a_cell() {
    let s = ChameleonState::new(4).unwrap();
    let mut moves = Vec::new();
    s.legal_moves(&mut moves);
    assert_eq!(moves.len(), 32);
    assert_eq!(s.move_index_bound(), 32);
    let blue = ChameleonMove {
        cell: game_hex::Cell(5),
        color: BLUE,
    };
    let red = ChameleonMove {
        cell: game_hex::Cell(5),
        color: RED,
    };
    assert_eq!((s.move_index(blue), s.move_index(red)), (10, 11));
}

#[test]
fn every_random_game_ends_with_a_winner_and_both_players_win() {
    // Pas de nul possible ; la règle « l'auteur gagne si les deux directions
    // sont reliées » et les chaînes des deux couleurs font gagner chaque camp.
    let mut rng = Pcg32::new(7, 7);
    let mut policy = UniformRollout::new();
    let mut trace = Trace::new(TraceMode::Off);
    let mut wins = [0u32; 2];
    for _ in 0..4000 {
        let mut state = ChameleonState::new(5).unwrap();
        match policy.rollout(&mut state, &mut rng, &mut trace) {
            Outcome::Win(p) => wins[p.index()] += 1,
            Outcome::Draw => panic!("nul impossible à Chameleon"),
        }
    }
    assert!(wins[0] > 1000 && wins[1] > 1000, "{wins:?}");
}

/// Directions (Ouest-Est, Nord-Sud) reliées par une chaîne de la couleur
/// `color`, recalculées par parcours en largeur.
fn connected(state: &ChameleonState, color: game_core::Player) -> (bool, bool) {
    let n = state.size();
    let at = |r: usize, c: usize| state.stone_at(game_hex::Cell::from_rc(r, c, n)) == Some(color);
    let reach = |starts: Vec<(usize, usize)>, goal: &dyn Fn(usize, usize) -> bool| {
        let mut seen = vec![false; n * n];
        let mut stack = starts;
        for &(r, c) in &stack {
            seen[r * n + c] = true;
        }
        while let Some((r, c)) = stack.pop() {
            if goal(r, c) {
                return true;
            }
            for (dr, dc) in [(-1isize, 0isize), (-1, 1), (0, 1), (1, 0), (1, -1), (0, -1)] {
                let (nr, nc) = (r as isize + dr, c as isize + dc);
                if nr >= 0 && nc >= 0 && (nr as usize) < n && (nc as usize) < n {
                    let (nr, nc) = (nr as usize, nc as usize);
                    if !seen[nr * n + nc] && at(nr, nc) {
                        seen[nr * n + nc] = true;
                        stack.push((nr, nc));
                    }
                }
            }
        }
        false
    };
    let west: Vec<_> = (0..n).filter(|&r| at(r, 0)).map(|r| (r, 0)).collect();
    let north: Vec<_> = (0..n).filter(|&c| at(0, c)).map(|c| (0, c)).collect();
    (
        reach(west, &|_, c| c == n - 1),
        reach(north, &|r, _| r == n - 1),
    )
}

#[test]
fn incremental_outcome_matches_a_full_recomputation() {
    let mut rng = Pcg32::new(21, 4);
    let mut buf = Vec::new();
    let mut both = 0;
    for game in 0..3000 {
        let size = 2 + game % 6;
        let mut state = ChameleonState::new(size).unwrap();
        while let Some(mv) = state.random_legal_move(&mut rng, &mut buf) {
            let mover = state.current_player();
            state.play(mv).unwrap();
            let (we, ns) = {
                let (b_we, b_ns) = connected(&state, BLUE);
                let (r_we, r_ns) = connected(&state, RED);
                (b_we || r_we, b_ns || r_ns)
            };
            let expected = match (we, ns) {
                (true, true) => {
                    both += 1;
                    Some(Outcome::Win(mover))
                }
                (true, false) => Some(Outcome::Win(BLUE)),
                (false, true) => Some(Outcome::Win(RED)),
                (false, false) => None,
            };
            assert_eq!(state.outcome(), expected, "{}", state.to_position());
        }
        assert!(state.outcome().is_some(), "plateau plein sans vainqueur");
    }
    assert!(
        both > 0,
        "le cas « deux directions d'un coup » doit apparaître"
    );
}
