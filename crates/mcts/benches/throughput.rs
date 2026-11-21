//! Banc de performance (remplace criterion, indisponible hors ligne).
//!
//! `cargo bench -p mcts` mesure, sur un cœur, le nombre de simulations par
//! seconde depuis le plateau vide 14 × 14 pour UCT et MC-RAVE, avec les deux
//! politiques de simulation. Chaque mesure est répétée et la médiane retenue.
//! Comme criterion, la recherche mesurée suit une recherche d'échauffement sur
//! le même moteur (mémoire de l'arbre déjà allouée, comme pour un agent d'un
//! coup à l'autre) ; le débit « froid » d'un moteur neuf est affiché à côté.
//! Variables d'environnement : `HEXBANDIT_BENCH_SIMS` (défaut 30 000),
//! `HEXBANDIT_BENCH_SIZE` (défaut 14), `HEXBANDIT_BENCH_REPEAT` (défaut 5).

use std::time::Instant;

use game_core::{
    rng::Pcg32,
    rollout::{RolloutPolicy, Trace, TraceMode},
};
use game_hex::{HexRollout, HexState};
use mcts::{
    CancelToken, MctsBuilder, Rave, TreeAlgorithm, Uct,
    beta::{AnySchedule, HandSelected, MinimumMse},
    budget::SimulationBudget,
};

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// Mesure une recherche de `sims` simulations depuis le plateau vide, moteur
/// chaud : comme criterion, une première recherche (non chronométrée) sert
/// d'échauffement, puis la recherche mesurée réutilise la mémoire de l'arbre,
/// comme un agent le fait d'un coup à l'autre. Renvoie (chaud, froid) : le
/// débit froid inclut l'allocation de l'arbre (fautes de page).
fn bench_search<A: TreeAlgorithm<HexState>>(
    label: &str,
    make: impl Fn() -> A,
    rollout: impl Fn() -> HexRollout,
    size: usize,
    sims: u64,
    repeat: u64,
) -> (f64, f64) {
    let root = HexState::new(size).unwrap();
    let mut warm = Vec::new();
    let mut cold = Vec::new();
    let mut depth = (0.0f32, 0u32, 0u32);
    for r in 0..repeat {
        let mut engine = MctsBuilder::new(make(), rollout())
            .max_nodes(2_000_000)
            .build()
            .unwrap();
        for (pass, rates) in [&mut cold, &mut warm].into_iter().enumerate() {
            let mut rng = Pcg32::new(r, 1 + pass as u64);
            let start = Instant::now();
            let report = engine
                .search(
                    &root,
                    &mut SimulationBudget { simulations: sims },
                    &mut rng,
                    &CancelToken::new(),
                )
                .unwrap();
            let secs = start.elapsed().as_secs_f64();
            rates.push(report.simulations as f64 / secs);
            depth = (report.mean_depth, report.max_depth, report.nodes);
        }
    }
    let (w, c) = (median(warm), median(cold));
    println!(
        "{label:<34} {w:>9.0} sim/s  (froid {c:>7.0})  profondeur moyenne {:.1}, max {}, {} nœuds",
        depth.0, depth.1, depth.2
    );
    (w, c)
}

fn bench_rollout(label: &str, mut policy: HexRollout, size: usize, mode: TraceMode, n: u64) {
    let root = HexState::new(size).unwrap();
    let mut scratch = root.clone();
    let mut rng = Pcg32::new(9, 9);
    let mut trace = Trace::new(mode);
    let start = Instant::now();
    let mut blue = 0u64;
    for _ in 0..n {
        scratch.clone_from(&root);
        trace.clear();
        if policy.rollout(&mut scratch, &mut rng, &mut trace)
            == game_core::Outcome::Win(game_hex::BLUE)
        {
            blue += 1;
        }
    }
    let secs = start.elapsed().as_secs_f64();
    println!(
        "{label:<34} {:>12.0} simulations/s   (Bleu gagne {:.1} %)",
        n as f64 / secs,
        100.0 * blue as f64 / n as f64
    );
}

fn main() {
    // `cargo bench` passe des arguments (--bench) que l'on ignore.
    let size = env("HEXBANDIT_BENCH_SIZE", 14) as usize;
    let sims = env("HEXBANDIT_BENCH_SIMS", 30_000);
    let repeat = env("HEXBANDIT_BENCH_REPEAT", 5);
    println!(
        "Plateau {size}x{size}, {sims} simulations depuis la position vide, médiane sur {repeat} essais\n\
         Recherche : moteur chaud (après une recherche d'échauffement, comme criterion) ;\n\
         « froid » = première recherche d'un moteur neuf, allocation de l'arbre comprise.\n"
    );
    println!("-- simulations seules (sans arbre) --");
    bench_rollout(
        "remplissage, sans trace",
        HexRollout::fill(),
        size,
        TraceMode::Off,
        200_000,
    );
    bench_rollout(
        "remplissage, trace exacte (RAVE)",
        HexRollout::fill(),
        size,
        TraceMode::Full,
        200_000,
    );
    bench_rollout(
        "uniforme coup par coup",
        HexRollout::uniform(),
        size,
        TraceMode::Off,
        100_000,
    );
    println!("\n-- recherche complète --");
    let (uct, _) = bench_search(
        "UCT + remplissage",
        Uct::default,
        HexRollout::fill,
        size,
        sims,
        repeat,
    );
    bench_search(
        "UCT + uniforme",
        Uct::default,
        HexRollout::uniform,
        size,
        sims,
        repeat,
    );
    let mse = || Rave::new(AnySchedule::MinimumMse(MinimumMse { bias: 0.05 }), 0.0);
    let (rave, _) = bench_search(
        "MC-RAVE (éq. 47) + remplissage",
        mse,
        HexRollout::fill,
        size,
        sims,
        repeat,
    );
    bench_search(
        "MC-RAVE (éq. 47) + uniforme",
        mse,
        HexRollout::uniform,
        size,
        sims,
        repeat,
    );
    let hand = || Rave::new(AnySchedule::HandSelected(HandSelected { k: 1000.0 }), 0.0);
    bench_search(
        "MC-RAVE + remplissage",
        hand,
        HexRollout::fill,
        size,
        sims,
        repeat,
    );
    println!();
    // Simulations uniformes, un cœur, 14 x 14, mesure de type criterion
    // (régime établi). La ligne MC-RAVE applique le même seuil : chaque
    // simulation y coûte en plus la trace exacte et la mise à jour AMAF.
    for (name, rate) in [("UCT", uct), ("MC-RAVE", rave)] {
        let verdict = if rate >= 300_000.0 {
            "objectif [S] 300 000 atteint"
        } else if rate >= 100_000.0 {
            "objectif [M] 100 000 atteint"
        } else {
            "SOUS l'objectif [M] de 100 000"
        };
        println!("{name:<8}: {rate:>9.0} simulations/s (moteur chaud) -> {verdict}");
    }
    println!(
        "\nLes débits dépendent fortement de la machine et de son mode d'énergie :\n\
         sur un portable, mesurer sur secteur, profil « performance », sans autre charge."
    );
}
