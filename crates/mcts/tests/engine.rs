//! Tests du moteur MCTS sur le Hex : invariants de l'arbre, déterminisme,
//! mises à jour AMAF, tactique élémentaire, limite mémoire, partie « golden ».

use game_core::{Game, Outcome, Player, rng::Pcg32};
use game_hex::{BLUE, Cell, HexRollout, HexState, RED, notation::parse_cell};
use mcts::{
    Agent, CancelToken, Mcts, MctsAgent, MctsBuilder, Rave, ReportDetail, SearchError, Uct,
    beta::{AnySchedule, HandSelected, MinimumMse, NoRave},
    budget::SimulationBudget,
    observer::{NullObserver, RecordingObserver},
    prior::EvenGame,
    runner::{MatchPlan, Seating, parallel_map, play_game, player_rng},
    tree::AmafStats,
};

fn uct_engine() -> Mcts<HexState, Uct, HexRollout> {
    MctsBuilder::new(Uct::default(), HexRollout::fill())
        .build()
        .unwrap()
}

fn rave_engine(schedule: AnySchedule) -> Mcts<HexState, Rave<AnySchedule>, HexRollout> {
    MctsBuilder::new(Rave::new(schedule, 0.0), HexRollout::fill())
        .detail(ReportDetail::Full)
        .build()
        .unwrap()
}

fn mse() -> AnySchedule {
    AnySchedule::MinimumMse(MinimumMse { bias: 0.05 })
}

/// Vérifie N(s) = Σ_a N(s,a) pour tout nœud non terminal.
fn check_visit_invariant<X: Copy + Default>(tree: &mcts::tree::Tree<Cell, X>) {
    for id in 0..tree.num_nodes() as u32 {
        let node = tree.node(id);
        if node.terminal.is_some() {
            continue;
        }
        let sum: u32 = tree.edges_of(id).iter().map(|e| e.n).sum();
        assert_eq!(node.visits, sum, "nœud {id}");
    }
}

#[test]
fn uct_tree_invariants() {
    let root = HexState::new(7).unwrap();
    let mut engine = uct_engine();
    let mut rng = Pcg32::new(1, 1);
    let report = engine
        .search(
            &root,
            &mut SimulationBudget { simulations: 3000 },
            &mut rng,
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(report.simulations, 3000);
    assert_eq!(engine.tree().node(0).visits, 3000);
    // Un nœud ajouté par simulation (plus la racine), sauf passages par un terminal.
    assert!(engine.tree().num_nodes() <= 3001);
    assert!(engine.tree().num_nodes() > 2900);
    check_visit_invariant(engine.tree());
    assert!(report.max_depth >= 2);
    assert!((0.0..=1.0).contains(&report.root_value));
}

#[test]
fn rave_tree_invariants_and_amaf_counts() {
    let root = HexState::new(6).unwrap();
    let mut engine = rave_engine(mse());
    let mut rng = Pcg32::new(2, 2);
    engine
        .search(
            &root,
            &mut SimulationBudget { simulations: 2000 },
            &mut rng,
            &CancelToken::new(),
        )
        .unwrap();
    check_visit_invariant(engine.tree());
    // Chaque simulation met à jour, à la racine, au moins la case jouée.
    let root_edges = engine.tree().edges_of(0);
    for e in root_edges {
        assert!(
            e.extra.n >= e.n,
            "Ñ(s,a) ≥ N(s,a) car a_t compte dans l'AMAF"
        );
        assert!(e.extra.n <= 2000);
    }
}

#[test]
fn single_simulation_amaf_partition() {
    // Après une seule simulation : la racine (Bleu) a Ñ = 1 exactement pour les
    // cases jouées par Bleu, le nœud créé (Rouge) pour celles de Rouge.
    let root = HexState::new(5).unwrap();
    let mut engine = rave_engine(mse());
    let mut rng = Pcg32::new(3, 3);
    engine
        .search(
            &root,
            &mut SimulationBudget { simulations: 1 },
            &mut rng,
            &CancelToken::new(),
        )
        .unwrap();
    let tree = engine.tree();
    assert_eq!(tree.num_nodes(), 2);
    assert_eq!(tree.node(0).player, BLUE);
    assert_eq!(tree.node(1).player, RED);
    let blue: Vec<Cell> = tree
        .edges_of(0)
        .iter()
        .filter(|e| e.extra.n == 1)
        .map(|e| e.mv)
        .collect();
    let red: Vec<Cell> = tree
        .edges_of(1)
        .iter()
        .filter(|e| e.extra.n == 1)
        .map(|e| e.mv)
        .collect();
    assert!(blue.iter().all(|c| !red.contains(c)), "un coup par case");
    assert!(blue.len() == red.len() || blue.len() == red.len() + 1);
    assert!(
        blue.len() >= 5,
        "il faut au moins 5 pierres pour relier 5×5"
    );
    // Le coup joué à la racine et le premier coup simulé sont comptés.
    let played_root = tree.edges_of(0).iter().find(|e| e.n == 1).unwrap();
    assert!(blue.contains(&played_root.mv));
    let played_child = tree.edges_of(1).iter().find(|e| e.n == 1).unwrap();
    assert!(red.contains(&played_child.mv));
    // Les valeurs AMAF valent la récompense de la simulation.
    let z_blue = played_root.q;
    for e in tree.edges_of(0).iter().filter(|e| e.extra.n == 1) {
        assert_eq!(e.extra, AmafStats { n: 1, q: z_blue });
    }
}

#[test]
fn search_is_deterministic() {
    let root = HexState::from_moves(9, BLUE, &[Cell(40), Cell(30)]).unwrap();
    for schedule in [mse(), AnySchedule::HandSelected(HandSelected { k: 1000.0 })] {
        let run = || {
            let mut engine = rave_engine(schedule);
            let mut rng = Pcg32::new(42, 7);
            let mut r = engine
                .search(
                    &root,
                    &mut SimulationBudget { simulations: 1500 },
                    &mut rng,
                    &CancelToken::new(),
                )
                .unwrap();
            r.elapsed_us = 0;
            r
        };
        assert_eq!(run(), run());
    }
}

/// Position 5×5 où c1 est l'unique coup gagnant pour le joueur au trait :
/// Bleu (a1 b1 d1 e1) et Rouge (c2 c3 c4 c5) ont tous deux besoin de c1.
fn blue_wins_in_one() -> (HexState, Cell) {
    let names = ["a1", "c2", "b1", "c3", "d1", "c4", "e1", "c5"];
    let moves: Vec<Cell> = names.iter().map(|m| parse_cell(m, 5).unwrap()).collect();
    let state = HexState::from_moves(5, BLUE, &moves).unwrap();
    (state, parse_cell("c1", 5).unwrap())
}

#[test]
fn engines_find_immediate_win() {
    let (state, win) = blue_wins_in_one();
    assert_eq!(state.current_player(), BLUE);
    let cancel = CancelToken::new();
    let mut rng = Pcg32::new(5, 5);
    let r = uct_engine()
        .search(
            &state,
            &mut SimulationBudget { simulations: 2000 },
            &mut rng,
            &cancel,
        )
        .unwrap();
    assert_eq!(r.best_move, win, "UCT");
    for schedule in [
        mse(),
        AnySchedule::HandSelected(HandSelected { k: 300.0 }),
        AnySchedule::None(NoRave),
    ] {
        let r = rave_engine(schedule)
            .search(
                &state,
                &mut SimulationBudget { simulations: 2000 },
                &mut rng,
                &cancel,
            )
            .unwrap();
        assert_eq!(r.best_move, win, "RAVE {schedule:?}");
        assert!(r.best_value > 0.9);
    }
}

#[test]
fn engines_block_immediate_loss() {
    // Rouge au trait : s'il ne joue pas c1, Bleu y gagne ; en c1, Rouge
    // menace b5 et c5 à la fois et gagne.
    let names = ["a1", "c2", "b1", "c3", "d1", "c4", "e1"];
    let moves: Vec<Cell> = names.iter().map(|m| parse_cell(m, 5).unwrap()).collect();
    let state = HexState::from_moves(5, BLUE, &moves).unwrap();
    assert_eq!(state.current_player(), RED);
    let block = parse_cell("c1", 5).unwrap();
    let mut rng = Pcg32::new(6, 6);
    for schedule in [mse(), AnySchedule::HandSelected(HandSelected { k: 1000.0 })] {
        let r = rave_engine(schedule)
            .search(
                &state,
                &mut SimulationBudget { simulations: 4000 },
                &mut rng,
                &CancelToken::new(),
            )
            .unwrap();
        assert_eq!(r.best_move, block, "RAVE {schedule:?}");
    }
    let r = uct_engine()
        .search(
            &state,
            &mut SimulationBudget { simulations: 4000 },
            &mut rng,
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(r.best_move, block);
}

#[test]
fn node_limit_stops_expansion_but_not_search() {
    let root = HexState::new(9).unwrap();
    let mut engine = MctsBuilder::new(Uct::default(), HexRollout::fill())
        .max_nodes(50)
        .build()
        .unwrap();
    let mut rng = Pcg32::new(8, 8);
    let r = engine
        .search(
            &root,
            &mut SimulationBudget { simulations: 500 },
            &mut rng,
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(r.simulations, 500);
    assert_eq!(engine.tree().num_nodes(), 50);
    assert!(r.node_limit_reached);
    check_visit_invariant(engine.tree());
}

#[test]
fn expand_threshold_reduces_nodes() {
    let root = HexState::new(7).unwrap();
    let mut engine = MctsBuilder::new(Uct::default(), HexRollout::fill())
        .expand_threshold(4)
        .build()
        .unwrap();
    let mut rng = Pcg32::new(9, 9);
    engine
        .search(
            &root,
            &mut SimulationBudget { simulations: 1000 },
            &mut rng,
            &CancelToken::new(),
        )
        .unwrap();
    assert!(engine.tree().num_nodes() < 400);
    assert!(
        MctsBuilder::<HexState, _, _>::new(Uct::default(), HexRollout::fill())
            .expand_threshold(0)
            .build()
            .is_err()
    );
}

#[test]
fn prior_knowledge_initialises_statistics() {
    let root = HexState::new(5).unwrap();
    let mut engine = MctsBuilder::new(Rave::new(mse(), 0.0), HexRollout::fill())
        .prior(Box::new(EvenGame { confidence: 10 }))
        .build()
        .unwrap();
    let mut rng = Pcg32::new(10, 10);
    engine
        .search(
            &root,
            &mut SimulationBudget { simulations: 200 },
            &mut rng,
            &CancelToken::new(),
        )
        .unwrap();
    check_visit_invariant(engine.tree());
    assert_eq!(engine.tree().node(0).visits, 25 * 10 + 200);
}

#[test]
fn cancellation_and_terminal_errors() {
    let (state, win) = blue_wins_in_one();
    let cancel = CancelToken::new();
    cancel.cancel();
    let mut rng = Pcg32::new(11, 11);
    let err = uct_engine().search(
        &state,
        &mut SimulationBudget { simulations: 10 },
        &mut rng,
        &cancel,
    );
    assert_eq!(err.unwrap_err(), SearchError::Cancelled);
    let mut done = state.clone();
    done.play(win).unwrap();
    let err = uct_engine().search(
        &done,
        &mut SimulationBudget { simulations: 10 },
        &mut rng,
        &CancelToken::new(),
    );
    assert_eq!(err.unwrap_err(), SearchError::TerminalPosition);
}

fn agent_uct(seed: u64, player: Player, sims: u64) -> MctsAgent<HexState, Uct, HexRollout> {
    MctsAgent::new(
        uct_engine(),
        Box::new(SimulationBudget { simulations: sims }),
        player_rng(seed, player),
        "uct".into(),
    )
}

fn agent_rave(
    seed: u64,
    player: Player,
    sims: u64,
) -> MctsAgent<HexState, Rave<AnySchedule>, HexRollout> {
    MctsAgent::new(
        rave_engine(mse()),
        Box::new(SimulationBudget { simulations: sims }),
        player_rng(seed, player),
        "rave".into(),
    )
}

fn moves_hash(moves: &[Cell]) -> u64 {
    // FNV-1a sur la suite des cases.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for c in moves {
        for b in c.0.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn golden_game(seed: u64) -> (Outcome, Vec<Cell>) {
    let mut blue = agent_rave(seed, BLUE, 300);
    let mut red = agent_uct(seed, RED, 300);
    let mut recorder = RecordingObserver::new(false);
    let (outcome, _) = play_game(
        HexState::new(6).unwrap(),
        [&mut blue as &mut dyn Agent<HexState>, &mut red],
        &mut recorder,
        &CancelToken::new(),
        100,
    )
    .unwrap();
    let record = recorder.into_record();
    (outcome, record.moves.iter().map(|m| m.mv).collect())
}

#[test]
fn games_are_reproducible_and_golden() {
    let (o1, m1) = golden_game(2026);
    let (o2, m2) = golden_game(2026);
    assert_eq!((o1, &m1), (o2, &m2));
    let (_, m3) = golden_game(2027);
    assert_ne!(m1, m3);
    // Valeur « golden » : toute modification du comportement du moteur ou des
    // règles change ce hash. Mettre à jour consciemment, avec un ADR si besoin.
    let hash = moves_hash(&m1);
    assert_eq!(hash, GOLDEN_HASH, "hash {hash:#x}, coups : {m1:?}");
}

const GOLDEN_HASH: u64 = 0x5d26_c68f_4ea9_a333;

#[test]
fn match_plan_alternates_and_parallel_is_thread_independent() {
    let plan = MatchPlan {
        master_seed: 77,
        n_games: 8,
        offset: 0,
        seating: Seating::AFirstAlternateColors,
    };
    let seats: Vec<_> = plan.seats().collect();
    assert!(seats.iter().all(|s| s.first_player == s.a_player));
    assert_eq!(seats.iter().filter(|s| s.a_player == BLUE).count(), 4);
    let plan2 = MatchPlan {
        offset: 4,
        n_games: 4,
        ..plan
    };
    assert_eq!(plan2.seat(0), plan.seat(4));
    let alt = MatchPlan {
        seating: Seating::AlternateFirst,
        ..plan
    };
    assert!(alt.seats().all(|s| s.first_player == BLUE));

    let run = |threads: usize| {
        parallel_map(6, threads, |i| {
            let seat = plan.seat(i as u32);
            let mut a = agent_uct(seat.seed, seat.a_player, 100);
            let mut b = agent_uct(seat.seed, seat.a_player.opponent(), 50);
            let (pa, pb): (&mut dyn Agent<HexState>, &mut dyn Agent<HexState>) = (&mut a, &mut b);
            let agents = if seat.a_player == BLUE {
                [pa, pb]
            } else {
                [pb, pa]
            };
            let initial = HexState::with_first_player(5, seat.first_player).unwrap();
            play_game(initial, agents, &mut NullObserver, &CancelToken::new(), 100)
                .unwrap()
                .0
        })
    };
    assert_eq!(run(1), run(2));
    assert_eq!(run(1), run(3));
}

#[test]
fn memory_layout_matches_adr_0004() {
    // Arbre en structure de tableaux : une arête occupe la somme des
    // colonnes tenues à jour ; 1/√N n'est stockée que si la sélection la lit.
    use mcts::tree::{Node, Tree};
    assert_eq!(std::mem::size_of::<Node>(), 16);
    let uct = uct_engine();
    assert_eq!(uct.tree().bytes_per_edge(), 18);
    let rave = rave_engine(mse());
    assert!(!rave.tree().keeps_inv_sqrt());
    assert_eq!(rave.tree().bytes_per_edge(), 22);
    let uct_rave = MctsBuilder::new(Rave::new(mse(), 0.3), HexRollout::fill())
        .build()
        .unwrap();
    assert_eq!(uct_rave.tree().bytes_per_edge(), 26);
    let mut tree: Tree<Cell, AmafStats> = Tree::new();
    tree.set_keep_inv_sqrt(false);
    assert_eq!(tree.bytes_per_edge(), 22);
}
