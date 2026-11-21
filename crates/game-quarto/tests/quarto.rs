//! Quarto avec le moteur générique : contrats, matchs nuls, UCT contre aléatoire.

use game_core::{
    Game, Outcome, Player,
    contract::{check_game_contract, check_rollout_contract},
    rng::Pcg32,
    rollout::UniformRollout,
};
use game_quarto::QuartoState;
use mcts::{
    Agent, CancelToken, MctsAgent, MctsBuilder, RandomAgent, Uct, budget::SimulationBudget,
    observer::NullObserver, runner::play_game,
};

#[test]
fn quarto_satisfies_contracts() {
    for squares in [false, true] {
        let s = QuartoState::new(Player::P1, squares);
        check_game_contract(&s, 1, 30, 17).unwrap();
        check_rollout_contract(&s, &mut UniformRollout::new(), 2, 100).unwrap();
    }
}

#[test]
fn random_games_include_draws_and_wins() {
    let mut rng = Pcg32::new(9, 9);
    let mut buf = Vec::new();
    let (mut draws, mut wins) = (0, 0);
    for _ in 0..2000 {
        let mut s = QuartoState::new(Player::P1, false);
        while let Some(mv) = s.random_legal_move(&mut rng, &mut buf) {
            s.play(mv).unwrap();
        }
        match s.outcome() {
            Some(Outcome::Draw) => draws += 1,
            Some(Outcome::Win(_)) => wins += 1,
            None => unreachable!(),
        }
        assert!(s.ply() <= 17);
    }
    assert!(draws > 50 && wins > 1000, "nuls {draws}, victoires {wins}");
}

#[test]
fn uct_beats_random_player() {
    let mut uct_wins = 0;
    let mut losses = 0;
    for game in 0..20u64 {
        let engine = MctsBuilder::new(
            Uct { exploration: 0.5 },
            UniformRollout::<QuartoState>::new(),
        )
        .build()
        .unwrap();
        let mut uct = MctsAgent::new(
            engine,
            Box::new(SimulationBudget { simulations: 2000 }),
            Pcg32::new(game, 1),
            "uct".into(),
        );
        let mut random = RandomAgent::<QuartoState>::new(Pcg32::new(game, 2));
        // UCT joue P1 dans les parties paires, P2 dans les impaires ; P1 commence.
        let uct_player = if game % 2 == 0 {
            Player::P1
        } else {
            Player::P2
        };
        let (a, b): (&mut dyn Agent<QuartoState>, &mut dyn Agent<QuartoState>) =
            (&mut uct, &mut random);
        let agents = if uct_player == Player::P1 {
            [a, b]
        } else {
            [b, a]
        };
        let (outcome, _) = play_game(
            QuartoState::new(Player::P1, false),
            agents,
            &mut NullObserver,
            &CancelToken::new(),
            40,
        )
        .unwrap();
        match outcome {
            Outcome::Win(p) if p == uct_player => uct_wins += 1,
            Outcome::Win(_) => losses += 1,
            Outcome::Draw => {}
        }
    }
    assert!(
        uct_wins >= 15 && losses <= 2,
        "UCT : {uct_wins} victoires, {losses} défaites"
    );
}
