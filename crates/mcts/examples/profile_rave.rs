//! Charge de travail pour le profilage (callgrind) de MC-RAVE.
use game_core::rng::Pcg32;
use game_hex::{HexRollout, HexState};
use mcts::{
    CancelToken, MctsBuilder, Rave, Uct,
    beta::{AnySchedule, MinimumMse},
    budget::SimulationBudget,
};
fn main() {
    let algo = std::env::args().nth(1).unwrap_or_else(|| "rave".into());
    let sims: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8000);
    let root = HexState::new(14).unwrap();
    let mut rng = Pcg32::new(1, 1);
    if algo == "rave" {
        let mut e = MctsBuilder::new(
            Rave::new(AnySchedule::MinimumMse(MinimumMse { bias: 0.05 }), 0.0),
            HexRollout::fill(),
        )
        .build()
        .unwrap();
        let r = e
            .search(
                &root,
                &mut SimulationBudget { simulations: sims },
                &mut rng,
                &CancelToken::new(),
            )
            .unwrap();
        println!("{} {}", r.simulations, r.mean_depth);
    } else {
        let mut e = MctsBuilder::new(Uct::default(), HexRollout::fill())
            .build()
            .unwrap();
        let r = e
            .search(
                &root,
                &mut SimulationBudget { simulations: sims },
                &mut rng,
                &CancelToken::new(),
            )
            .unwrap();
        println!("{} {}", r.simulations, r.mean_depth);
    }
}
