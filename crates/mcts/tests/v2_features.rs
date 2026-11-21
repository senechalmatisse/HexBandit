//! Options : MCTS-Solver, réutilisation de l'arbre, GRAVE et parallélisme à la racine.
//! Désactivées, elles ne changent rien (voir `fingerprint.rs` et la partie « golden » de `engine.rs`) ;
//! activées, elles doivent rester correctes et déterministes.

use std::collections::HashMap;

use game_core::{Game, GameError, Outcome, Player, rng::Pcg32, rollout::UniformRollout};
use game_hex::{BLUE, Cell, HexRollout, HexState, notation::parse_cell};
use mcts::{
    Agent, CancelToken, Mcts, MctsAgent, MctsBuilder, Rave, ReportDetail, RootParallelAgent,
    TreeAlgorithm, Uct,
    beta::{AnySchedule, MinimumMse},
    budget::SimulationBudget,
    observer::{NullObserver, RecordingObserver},
    runner::{play_game, player_rng},
    tree::{NO_NODE, Tree},
};

fn mse() -> AnySchedule {
    AnySchedule::MinimumMse(MinimumMse { bias: 0.02 })
}

fn sims(n: u64) -> SimulationBudget {
    SimulationBudget { simulations: n }
}

/// Solveur exact de référence (minimax mémoïsé), en demi-points pour le
/// joueur au trait : 0 défaite, 1 nul, 2 victoire.
fn exact_value<G: Game>(state: &G, key: &dyn Fn(&G) -> u64, memo: &mut HashMap<u64, u8>) -> u8 {
    if let Some(o) = state.outcome() {
        return (2.0 * o.reward_for(state.current_player())) as u8;
    }
    let k = key(state);
    if let Some(&v) = memo.get(&k) {
        return v;
    }
    let me = state.current_player();
    let mut moves = Vec::new();
    state.legal_moves(&mut moves);
    let mut best = 0u8;
    for mv in moves {
        let mut next = state.clone();
        next.play(mv).unwrap();
        let v = exact_value(&next, key, memo);
        let mine = if next.current_player() == me {
            v
        } else {
            2 - v
        };
        best = best.max(mine);
        if best == 2 {
            break;
        }
    }
    memo.insert(k, best);
    best
}

/// Vérifie, pour chaque nœud de l'arbre, que la valeur exacte de sa position
/// est dans ses bornes prouvées. Renvoie le nombre de nœuds résolus.
fn check_proofs<G: Game, X: Copy + Default>(
    tree: &Tree<G::Move, X>,
    root: &G,
    key: &dyn Fn(&G) -> u64,
    memo: &mut HashMap<u64, u8>,
) -> usize {
    let mut solved = 0;
    let mut stack = vec![(0u32, root.clone())];
    while let Some((id, state)) = stack.pop() {
        let node = tree.node(id);
        let exact = exact_value(&state, key, memo);
        let (lo, hi) = (node.proof.lower(), node.proof.upper());
        assert!(
            lo <= exact && exact <= hi,
            "nœud {id} : exact {exact} hors de [{lo}, {hi}]"
        );
        solved += usize::from(node.proof.is_solved());
        let edges = tree.edges_of(id);
        for (i, &child) in edges.children.iter().enumerate() {
            if child != NO_NODE {
                let mut next = state.clone();
                next.play(edges.moves[i]).unwrap();
                stack.push((child, next));
            }
        }
    }
    solved
}

/// Clé compacte d'une position de Hex : cases en base 3, puis le trait.
fn hex_key(s: &HexState) -> u64 {
    let n = s.size();
    let mut key = 0u64;
    for cell in 0..n * n {
        let code = match s.stone_at(Cell(cell as u16)) {
            None => 0,
            Some(p) => 1 + p.index() as u64,
        };
        key = key * 3 + code;
    }
    key * 2 + s.current_player().index() as u64
}

fn random_position(size: usize, plies: usize, seed: u64) -> HexState {
    let mut rng = Pcg32::new(seed, 99);
    let mut state = HexState::new(size).unwrap();
    let mut buf = Vec::new();
    for _ in 0..plies {
        let before = state.clone();
        let Some(mv) = state.random_legal_move(&mut rng, &mut buf) else {
            break;
        };
        state.play(mv).unwrap();
        if state.is_terminal() {
            return before;
        }
    }
    state
}

#[test]
fn solver_bounds_are_sound_on_small_hex_boards() {
    let mut total_solved = 0;
    let mut memo = HashMap::new();
    for seed in 0..12u64 {
        // 3 × 3 depuis le début de partie ; 4 × 4 après quelques coups (la
        // résolution exacte de référence reste rapide).
        let (size, plies) = if seed % 2 == 0 {
            (3, seed % 5)
        } else {
            (4, 4 + seed % 3)
        };
        let root = random_position(size, plies as usize, seed);
        let mut uct = MctsBuilder::new(Uct { exploration: 0.25 }, HexRollout::fill())
            .solver(true)
            .build()
            .unwrap();
        let mut rng = Pcg32::new(seed, 1);
        let r = uct
            .search(&root, &mut sims(3000), &mut rng, &CancelToken::new())
            .unwrap();
        total_solved += check_proofs(uct.tree(), &root, &hex_key, &mut memo);
        if let Some(v) = r.proven {
            assert_eq!(v, exact_value(&root, &hex_key, &mut memo), "graine {seed}");
        }
        let mut rave = MctsBuilder::new(Rave::new(mse(), 0.0), HexRollout::fill())
            .solver(true)
            .build()
            .unwrap();
        let r = rave
            .search(&root, &mut sims(3000), &mut rng, &CancelToken::new())
            .unwrap();
        total_solved += check_proofs(rave.tree(), &root, &hex_key, &mut memo);
        if let Some(v) = r.proven {
            assert_eq!(v, exact_value(&root, &hex_key, &mut memo), "graine {seed}");
        }
    }
    assert!(
        total_solved > 100,
        "le solveur doit résoudre des nœuds ({total_solved})"
    );
}

#[test]
fn solver_proves_the_first_player_win_on_3x3_and_stops_early() {
    let root = HexState::new(3).unwrap();
    let mut engine = MctsBuilder::new(Rave::new(mse(), 0.0), HexRollout::fill())
        .solver(true)
        .detail(ReportDetail::Full)
        .build()
        .unwrap();
    let mut rng = Pcg32::new(3, 3);
    let r = engine
        .search(&root, &mut sims(50_000), &mut rng, &CancelToken::new())
        .unwrap();
    assert_eq!(r.proven, Some(2), "Hex 3×3 : le premier joueur gagne");
    assert!(
        r.simulations < 50_000,
        "arrêt dès que la racine est résolue"
    );
    // Le coup joué est un coup prouvé gagnant.
    let best = r.candidates.iter().find(|c| c.mv == r.best_move).unwrap();
    assert_eq!(best.proven, Some(2));
    // Sans solveur, rien n'est prouvé et tout le budget est utilisé.
    let mut plain = MctsBuilder::new(Rave::new(mse(), 0.0), HexRollout::fill())
        .build()
        .unwrap();
    let r = plain
        .search(&root, &mut sims(5_000), &mut rng, &CancelToken::new())
        .unwrap();
    assert_eq!((r.proven, r.simulations), (None, 5_000));
}

#[test]
fn solver_plays_the_forced_win_and_avoids_proven_losses() {
    // c1 est l'unique coup gagnant de Bleu (voir engine.rs).
    let names = ["a1", "c2", "b1", "c3", "d1", "c4", "e1", "c5"];
    let moves: Vec<Cell> = names.iter().map(|m| parse_cell(m, 5).unwrap()).collect();
    let state = HexState::from_moves(5, BLUE, &moves).unwrap();
    let mut engine = MctsBuilder::new(Uct { exploration: 0.25 }, HexRollout::fill())
        .solver(true)
        .build()
        .unwrap();
    let mut rng = Pcg32::new(4, 4);
    let r = engine
        .search(&state, &mut sims(500), &mut rng, &CancelToken::new())
        .unwrap();
    assert_eq!(r.best_move, parse_cell("c1", 5).unwrap());
    assert_eq!(r.proven, Some(2));
}

/// Morpion : jeu à matchs nuls, pour vérifier la gestion des nuls par les
/// bornes du solveur.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TicTacToe {
    cells: [u8; 9],
    to_move: Player,
    ply: u32,
    outcome: Option<Outcome>,
}

impl TicTacToe {
    fn new() -> Self {
        TicTacToe {
            cells: [0; 9],
            to_move: Player::P1,
            ply: 0,
            outcome: None,
        }
    }
}

impl Game for TicTacToe {
    type Move = u8;
    fn current_player(&self) -> Player {
        self.to_move
    }
    fn legal_moves(&self, out: &mut Vec<u8>) {
        out.clear();
        if self.outcome.is_none() {
            out.extend((0..9u8).filter(|&c| self.cells[usize::from(c)] == 0));
        }
    }
    fn play(&mut self, mv: u8) -> Result<(), GameError> {
        if self.outcome.is_some() {
            return Err(GameError::GameOver);
        }
        if mv >= 9 || self.cells[usize::from(mv)] != 0 {
            return Err(GameError::IllegalMove {
                mv: mv.to_string(),
                reason: "case".into(),
            });
        }
        let code = 1 + self.to_move.index() as u8;
        self.cells[usize::from(mv)] = code;
        const LINES: [[usize; 3]; 8] = [
            [0, 1, 2],
            [3, 4, 5],
            [6, 7, 8],
            [0, 3, 6],
            [1, 4, 7],
            [2, 5, 8],
            [0, 4, 8],
            [2, 4, 6],
        ];
        if LINES
            .iter()
            .any(|l| l.iter().all(|&c| self.cells[c] == code))
        {
            self.outcome = Some(Outcome::Win(self.to_move));
        } else if self.cells.iter().all(|&c| c != 0) {
            self.outcome = Some(Outcome::Draw);
        }
        self.to_move = self.to_move.opponent();
        self.ply += 1;
        Ok(())
    }
    fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }
    fn ply(&self) -> u32 {
        self.ply
    }
}

#[test]
fn solver_handles_draws() {
    let key = |s: &TicTacToe| {
        s.cells.iter().fold(0u64, |k, &c| k * 3 + u64::from(c)) * 2 + s.to_move.index() as u64
    };
    let mut memo = HashMap::new();
    let root = TicTacToe::new();
    let mut engine = MctsBuilder::new(Uct { exploration: 0.7 }, UniformRollout::new())
        .solver(true)
        .build()
        .unwrap();
    let mut rng = Pcg32::new(5, 5);
    let r = engine
        .search(&root, &mut sims(200_000), &mut rng, &CancelToken::new())
        .unwrap();
    let solved = check_proofs(engine.tree(), &root, &key, &mut memo);
    assert!(solved > 1000, "{solved} nœuds résolus");
    assert_eq!(r.proven, Some(1), "le morpion est nul en jeu parfait");
    // Position où le joueur au trait (X) gagne : X en 0 et 1, O en 3 et 4.
    let mut won = TicTacToe::new();
    for mv in [0, 3, 1, 4] {
        won.play(mv).unwrap();
    }
    let r = engine
        .search(&won, &mut sims(2_000), &mut rng, &CancelToken::new())
        .unwrap();
    assert_eq!((r.best_move, r.proven), (2, Some(2)));
    check_proofs(engine.tree(), &won, &key, &mut memo);
}

// ---------------------------------------------------------------------------
// Réutilisation de l'arbre
// ---------------------------------------------------------------------------

/// N(s) = Σ N(s,a) pour tout nœud non terminal et non résolu.
fn check_visits<X: Copy + Default>(tree: &Tree<Cell, X>) {
    for id in 0..tree.num_nodes() as u32 {
        let node = tree.node(id);
        if node.terminal.is_some() || node.proof.is_solved() {
            continue;
        }
        let sum: u32 = tree.edges_of(id).visits.iter().sum();
        assert_eq!(node.visits, sum, "nœud {id}");
    }
}

fn reuse_engine<A: TreeAlgorithm<HexState>>(algorithm: A) -> Mcts<HexState, A, HexRollout> {
    MctsBuilder::new(algorithm, HexRollout::fill())
        .reuse_tree(true)
        .build()
        .unwrap()
}

#[test]
fn reused_subtree_keeps_the_statistics_of_the_played_line() {
    let root = HexState::new(6).unwrap();
    let mut engine = reuse_engine(Rave::new(mse(), 0.0));
    let mut rng = Pcg32::new(6, 6);
    let cancel = CancelToken::new();
    let first = engine
        .search_after(&root, &[], &mut sims(4000), &mut rng, &cancel)
        .unwrap();
    // Statistiques attendues : celles du petit-enfant le long (coup, réponse).
    let tree = engine.tree();
    let edges = tree.edges_of(0);
    let i = edges
        .moves
        .iter()
        .position(|&m| m == first.best_move)
        .unwrap();
    let child = edges.children[i];
    let reply_edges = tree.edges_of(child);
    let j = (0..reply_edges.len())
        .max_by_key(|&j| reply_edges.visits[j])
        .unwrap();
    let (reply, grandchild) = (reply_edges.moves[j], reply_edges.children[j]);
    assert_ne!(grandchild, NO_NODE);
    let expected_visits = tree.node(grandchild).visits;
    let expected_edges: Vec<(Cell, u32)> = tree
        .edges_of(grandchild)
        .iter()
        .map(|e| (e.mv, e.n))
        .collect();
    let mut next = root.clone();
    next.play(first.best_move).unwrap();
    next.play(reply).unwrap();
    let second = engine
        .search_after(
            &next,
            &[first.best_move, reply],
            &mut sims(1000),
            &mut rng,
            &cancel,
        )
        .unwrap();
    assert_eq!(second.reused_visits, expected_visits);
    assert_eq!(engine.tree().node(0).visits, expected_visits + 1000);
    let after: Vec<(Cell, u32)> = engine
        .tree()
        .edges_of(0)
        .iter()
        .map(|e| (e.mv, e.n))
        .collect();
    assert_eq!(after.len(), expected_edges.len());
    for ((m0, n0), (m1, n1)) in expected_edges.iter().zip(&after) {
        assert_eq!(m0, m1, "ordre des arêtes conservé");
        assert!(n1 >= n0);
    }
    check_visits(engine.tree());
    // Un chemin absent de l'arbre, ou sans réutilisation : arbre neuf.
    let third = engine
        .search(&next, &mut sims(500), &mut rng, &cancel)
        .unwrap();
    assert_eq!(third.reused_visits, 0);
    assert_eq!(engine.tree().node(0).visits, 500);
}

#[test]
fn games_with_tree_reuse_are_reproducible() {
    let play = |seed: u64| {
        let make = |player: Player| {
            MctsAgent::new(
                reuse_engine(Rave::new(mse(), 0.0)),
                Box::new(sims(300)),
                player_rng(seed, player),
                "rave-reuse".into(),
            )
        };
        let (mut blue, mut red) = (make(Player::P1), make(Player::P2));
        let mut recorder = RecordingObserver::new(true);
        let (outcome, _) = play_game(
            HexState::new(6).unwrap(),
            [&mut blue as &mut dyn Agent<HexState>, &mut red],
            &mut recorder,
            &CancelToken::new(),
            100,
        )
        .unwrap();
        let record = recorder.into_record();
        let reused: u32 = record
            .moves
            .iter()
            .map(|m| m.report.as_ref().unwrap().reused_visits)
            .sum();
        (
            outcome,
            record.moves.iter().map(|m| m.mv).collect::<Vec<_>>(),
            reused,
        )
    };
    let (o1, m1, reused) = play(11);
    assert_eq!((o1, m1.clone(), reused), play(11));
    assert!(reused > 0, "l'arbre doit être réutilisé");
}

// ---------------------------------------------------------------------------
// GRAVE
// ---------------------------------------------------------------------------

#[test]
fn grave_finds_tactics_and_keeps_invariants() {
    let names = ["a1", "c2", "b1", "c3", "d1", "c4", "e1", "c5"];
    let moves: Vec<Cell> = names.iter().map(|m| parse_cell(m, 5).unwrap()).collect();
    let state = HexState::from_moves(5, BLUE, &moves).unwrap();
    let mut engine = MctsBuilder::new(Rave::new(mse(), 0.0).with_grave(50), HexRollout::fill())
        .build()
        .unwrap();
    assert_eq!(TreeAlgorithm::<HexState>::name(engine.algorithm()), "grave");
    let mut rng = Pcg32::new(7, 7);
    let r = engine
        .search(&state, &mut sims(3000), &mut rng, &CancelToken::new())
        .unwrap();
    assert_eq!(r.best_move, parse_cell("c1", 5).unwrap());
    check_visits(engine.tree());
    // GRAVE et MC-RAVE diffèrent dès que le seuil sert.
    let root = HexState::new(7).unwrap();
    let run = |grave: u32| {
        let mut e = MctsBuilder::new(Rave::new(mse(), 0.0).with_grave(grave), HexRollout::fill())
            .build()
            .unwrap();
        let mut rng = Pcg32::new(8, 8);
        e.search(&root, &mut sims(2000), &mut rng, &CancelToken::new())
            .unwrap();
        e.tree().edges_of(0).visits.to_vec()
    };
    assert_ne!(run(0), run(100));
    assert_eq!(run(0), run(0));
}

// ---------------------------------------------------------------------------
// Parallélisme à la racine
// ---------------------------------------------------------------------------

fn parallel_agent(
    trees: usize,
    seed: u64,
) -> RootParallelAgent<HexState, Rave<AnySchedule>, HexRollout> {
    let workers = (0..trees)
        .map(|_| {
            MctsBuilder::new(Rave::new(mse(), 0.0), HexRollout::fill())
                .detail(ReportDetail::Full)
                .solver(true)
                .build()
                .unwrap()
        })
        .collect();
    RootParallelAgent::new(
        workers,
        Box::new(sims(3001)),
        player_rng(seed, BLUE),
        "par".into(),
    )
}

#[test]
fn root_parallel_search_is_deterministic_and_merges_statistics() {
    let root = HexState::from_moves(7, BLUE, &[Cell(24), Cell(17)]).unwrap();
    let run = || {
        let mut agent = parallel_agent(3, 12);
        assert_eq!(agent.trees(), 3);
        let mut r = agent.choose(&root, &CancelToken::new()).unwrap();
        r.elapsed_us = 0;
        r
    };
    let (a, b) = (run(), run());
    assert_eq!(a, b, "indépendant de l'ordonnancement des threads");
    assert_eq!(a.simulations, 3001);
    let total: u32 = a.candidates.iter().map(|c| c.visits).sum();
    assert_eq!(u64::from(total), 3001);
    assert_eq!(a.candidates.len(), 47);
    // Partie complète : le joueur parallèle bat-il au moins le hasard ?
    let mut blue = parallel_agent(2, 3);
    let mut red = mcts::RandomAgent::new(Pcg32::new(1, 1));
    let (outcome, _) = play_game(
        HexState::new(5).unwrap(),
        [&mut blue as &mut dyn Agent<HexState>, &mut red],
        &mut NullObserver,
        &CancelToken::new(),
        100,
    )
    .unwrap();
    assert_eq!(outcome, Outcome::Win(BLUE));
}
