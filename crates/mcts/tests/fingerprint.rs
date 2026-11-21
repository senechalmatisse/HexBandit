//! Empreintes de l'arbre complet après une recherche (statistiques de chaque
//! nœud et de chaque arête, au bit près) pour plusieurs configurations.
//!
//! Les campagnes enregistrées restent rejouables à l'identique (`hexbandit verify`).

use game_core::{
    Game,
    decorators::{Misere, MisereRollout, SwapMove, SwapRollout, SwapRule},
    rng::Pcg32,
    rollout::RolloutPolicy,
};
use game_hex::{Cell, HexRollout, HexState, notation::parse_cell};
use mcts::{
    CancelToken, MctsBuilder, Rave, TreeAlgorithm, Uct,
    beta::{AnySchedule, HandSelected, MinimumMse, NoRave},
    budget::SimulationBudget,
    prior::EvenGame,
};

/// FNV-1a sur 64 bits.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn word(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

#[derive(Clone, Copy)]
struct Options {
    sims: u64,
    seed: u64,
    max_nodes: u32,
    expand: u32,
    prior: Option<u32>,
}

impl Options {
    fn new(sims: u64, seed: u64) -> Self {
        Options {
            sims,
            seed,
            max_nodes: 250_000,
            expand: 1,
            prior: None,
        }
    }
}

/// Recherche puis empreinte de tout l'arbre.
fn fingerprint<G, A, P>(
    root: &G,
    algorithm: A,
    rollout: P,
    opts: Options,
    key: impl Fn(G::Move) -> u64,
) -> u64
where
    G: Game + 'static,
    A: TreeAlgorithm<G>,
    P: RolloutPolicy<G>,
{
    let mut builder = MctsBuilder::new(algorithm, rollout)
        .max_nodes(opts.max_nodes)
        .expand_threshold(opts.expand);
    if let Some(confidence) = opts.prior {
        builder = builder.prior(Box::new(EvenGame { confidence }));
    }
    let mut engine = builder.build().unwrap();
    let mut rng = Pcg32::new(opts.seed, 7);
    let report = engine
        .search(
            root,
            &mut SimulationBudget {
                simulations: opts.sims,
            },
            &mut rng,
            &CancelToken::new(),
        )
        .unwrap();
    let tree = engine.tree();
    let mut h = Fnv::new();
    h.word(key(report.best_move));
    for id in 0..tree.num_nodes() as u32 {
        let node = tree.node(id);
        h.word(u64::from(node.visits));
        h.word(u64::from(node.first_edge));
        h.word(u64::from(node.num_edges));
        for e in tree.edges_of(id) {
            h.word(key(e.mv));
            h.word(u64::from(e.child));
            h.word(u64::from(e.n));
            h.word(u64::from(e.q.to_bits()));
            h.word(u64::from(e.inv_sqrt_n.to_bits()));
            if let Some(amaf) = engine.algorithm().amaf(&e) {
                h.word(u64::from(amaf.n));
                h.word(u64::from(amaf.q.to_bits()));
            }
        }
    }
    h.0
}

fn hex_after(size: usize, moves: &str) -> HexState {
    let mut state = HexState::new(size).unwrap();
    for text in moves.split(',').filter(|m| !m.is_empty()) {
        state.play(parse_cell(text, size).unwrap()).unwrap();
    }
    state
}

fn cell(mv: Cell) -> u64 {
    u64::from(mv.0)
}

fn swap_key(mv: SwapMove<Cell>) -> u64 {
    match mv {
        SwapMove::Place(c) => u64::from(c.0),
        SwapMove::Swap => u64::MAX,
    }
}

fn mse(bias: f32) -> AnySchedule {
    AnySchedule::MinimumMse(MinimumMse { bias })
}

#[test]
fn tree_statistics_are_unchanged_by_optimisations() {
    let cases: Vec<(&str, u64)> = vec![
        (
            "rave-mse-9x9",
            fingerprint(
                &HexState::new(9).unwrap(),
                Rave::new(mse(0.02), 0.0),
                HexRollout::fill(),
                Options::new(3000, 1),
                cell,
            ),
        ),
        (
            "rave-manual-7x7",
            fingerprint(
                &hex_after(7, "d4,c5,e3"),
                Rave::new(AnySchedule::HandSelected(HandSelected { k: 1000.0 }), 0.0),
                HexRollout::fill(),
                Options::new(3000, 2),
                cell,
            ),
        ),
        (
            "uct-rave-uniform-6x6",
            fingerprint(
                &HexState::new(6).unwrap(),
                Rave::new(mse(0.05), 0.3),
                HexRollout::uniform(),
                Options::new(2000, 3),
                cell,
            ),
        ),
        (
            "rave-none-5x5",
            fingerprint(
                &HexState::new(5).unwrap(),
                Rave::new(AnySchedule::None(NoRave), 0.0),
                HexRollout::fill(),
                Options::new(1000, 4),
                cell,
            ),
        ),
        (
            "rave-prior-7x7",
            fingerprint(
                &HexState::new(7).unwrap(),
                Rave::new(mse(0.02), 0.0),
                HexRollout::fill(),
                Options {
                    prior: Some(10),
                    ..Options::new(2000, 5)
                },
                cell,
            ),
        ),
        (
            "rave-limits-9x9",
            fingerprint(
                &HexState::new(9).unwrap(),
                Rave::new(mse(0.02), 0.0),
                HexRollout::fill(),
                Options {
                    max_nodes: 300,
                    expand: 2,
                    ..Options::new(3000, 6)
                },
                cell,
            ),
        ),
        (
            "rave-reverse-6x6",
            fingerprint(
                &Misere::new(HexState::new(6).unwrap()),
                Rave::new(mse(0.02), 0.0),
                MisereRollout::new(HexRollout::fill()),
                Options::new(2000, 7),
                cell,
            ),
        ),
        (
            "rave-swap-5x5",
            fingerprint(
                &swap_after_first_move(),
                Rave::new(mse(0.02), 0.0),
                SwapRollout::new(HexRollout::fill()),
                Options::new(2000, 8),
                swap_key,
            ),
        ),
        (
            "uct-9x9",
            fingerprint(
                &HexState::new(9).unwrap(),
                Uct { exploration: 0.25 },
                HexRollout::fill(),
                Options::new(3000, 9),
                cell,
            ),
        ),
    ];
    let expected: [(&str, u64); 9] = EXPECTED;
    let mut failures = Vec::new();
    for ((name, got), (want_name, want)) in cases.iter().zip(expected) {
        assert_eq!(*name, want_name);
        if *got != want {
            failures.push(format!("{name}: {got:#018x} (attendu {want:#018x})"));
        }
    }
    assert!(
        failures.is_empty(),
        "empreintes modifiées :\n{}",
        failures.join("\n")
    );
}

fn swap_after_first_move() -> SwapRule<HexState> {
    let mut state = SwapRule::new(HexState::new(5).unwrap());
    state.play(SwapMove::Place(Cell(7))).unwrap();
    state
}

const EXPECTED: [(&str, u64); 9] = [
    ("rave-mse-9x9", 0x8b8c_5b35_b768_96f0),
    ("rave-manual-7x7", 0x7f8d_a485_8c87_165f),
    ("uct-rave-uniform-6x6", 0x8b4f_c735_e337_2f0c),
    ("rave-none-5x5", 0x14c9_3243_89d1_66eb),
    ("rave-prior-7x7", 0xf5fa_6f13_64dd_d399),
    ("rave-limits-9x9", 0x2207_8599_8c74_25b0),
    ("rave-reverse-6x6", 0xe9b2_d5c2_82af_bfa4),
    ("rave-swap-5x5", 0x6a8f_a5e3_338d_52e7),
    ("uct-9x9", 0x3c8f_1bfa_0e6c_135f),
];
