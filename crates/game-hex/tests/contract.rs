//! Tests de contrat : toutes les implémentations de `Game`,
//! `AmafGame` et `RolloutPolicy` du crate respectent les contrats génériques.

use game_core::{
    contract::{check_amaf_contract, check_game_contract, check_rollout_contract},
    decorators::{Misere, MisereRollout, SwapRollout, SwapRule},
};
use game_hex::{HexRollout, HexState, RED, ReverseHex};

#[test]
fn hex_state_contract() {
    for size in [2, 3, 5, 9, 14, 19] {
        let s = HexState::new(size).unwrap();
        check_game_contract(&s, size as u64, 20, size * size).unwrap();
        check_amaf_contract(&s, 1, 10).unwrap();
    }
    let s = HexState::with_first_player(6, RED).unwrap();
    check_game_contract(&s, 99, 20, 36).unwrap();
}

#[test]
fn decorated_variants_contract() {
    let hex = HexState::new(5).unwrap();
    check_game_contract(&ReverseHex::new(hex.clone()), 2, 30, 25).unwrap();
    check_amaf_contract(&ReverseHex::new(hex.clone()), 3, 10).unwrap();
    check_game_contract(&SwapRule::new(hex.clone()), 4, 30, 26).unwrap();
    check_amaf_contract(&SwapRule::new(hex.clone()), 5, 10).unwrap();
    check_game_contract(&SwapRule::new(Misere::new(hex)), 6, 30, 26).unwrap();
}

#[test]
fn rollout_policies_contract() {
    for size in [2, 3, 4, 7, 11, 14] {
        let s = HexState::new(size).unwrap();
        check_rollout_contract(&s, &mut HexRollout::fill(), 10 + size as u64, 200).unwrap();
        check_rollout_contract(&s, &mut HexRollout::uniform(), 20 + size as u64, 50).unwrap();
    }
    let s = HexState::new(6).unwrap();
    check_rollout_contract(
        &ReverseHex::new(s.clone()),
        &mut MisereRollout::new(HexRollout::fill()),
        7,
        100,
    )
    .unwrap();
    check_rollout_contract(
        &SwapRule::new(s.clone()),
        &mut SwapRollout::new(HexRollout::fill()),
        8,
        100,
    )
    .unwrap();
    check_rollout_contract(
        &SwapRule::new(Misere::new(s)),
        &mut SwapRollout::new(MisereRollout::new(HexRollout::fill())),
        9,
        100,
    )
    .unwrap();
}
