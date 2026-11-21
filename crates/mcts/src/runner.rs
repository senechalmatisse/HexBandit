//! Orchestration des parties et des matchs.

use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

use game_core::{
    Game, Outcome, Player,
    rng::{Pcg32, derive_seed},
};

use crate::{agent::Agent, cancel::CancelToken, observer::MatchObserver, report::SearchError};

/// Joue une partie complète entre deux joueurs, indexés par identité
/// (`agents[0]` joue [`Player::P1`], `agents[1]` joue [`Player::P2`]).
///
/// # Errors
///
/// Erreur d'un joueur, coup illégal, annulation ou dépassement de `max_plies`.
pub fn play_game<G: Game>(
    initial: G,
    mut agents: [&mut dyn Agent<G>; 2],
    observer: &mut dyn MatchObserver<G>,
    cancel: &CancelToken,
    max_plies: u32,
) -> Result<(Outcome, G), SearchError> {
    let mut state = initial;
    observer.on_start(&state);
    loop {
        if let Some(outcome) = state.outcome() {
            observer.on_end(outcome, &state);
            return Ok((outcome, state));
        }
        if cancel.is_cancelled() {
            return Err(SearchError::Cancelled);
        }
        if state.ply() >= max_plies {
            return Err(SearchError::PlyLimit(max_plies));
        }
        let player = state.current_player();
        let report = agents[player.index()].choose(&state, cancel)?;
        state.play(report.best_move)?;
        for agent in &mut agents {
            agent.observe(report.best_move);
        }
        observer.on_move(state.ply(), player, report.best_move, &report);
    }
}

/// Placement des deux joueurs A et B au fil d'un match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seating {
    /// Le premier joueur est toujours `P1` ; A est `P1` dans les parties
    /// paires, `P2` dans les impaires : A et B commencent chacun une partie
    /// sur deux (match UCT contre RAVE).
    AlternateFirst,
    /// A commence toujours ; sa couleur alterne d'une partie à l'autre
    /// (expérience sur le rapport de budget : A est le premier joueur).
    AFirstAlternateColors,
}

/// Placement d'une partie d'un match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameSeat {
    /// Indice global de la partie dans le match.
    pub index: u64,
    /// Graine de la partie, dérivée de la graine maître.
    pub seed: u64,
    /// Identité (couleur) jouée par A.
    pub a_player: Player,
    /// Joueur qui commence.
    pub first_player: Player,
}

/// Plan d'un match de `n_games` parties : alternance du premier
/// joueur et graines dérivées de façon déterministe de la graine maître.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchPlan {
    /// Graine maître.
    pub master_seed: u64,
    /// Nombre de parties.
    pub n_games: u32,
    /// Indice de la première partie (pour exécuter un match par tranches).
    pub offset: u64,
    /// Règle de placement.
    pub seating: Seating,
}

impl MatchPlan {
    /// Placement de la `i`-ème partie du plan.
    #[must_use]
    pub fn seat(&self, i: u32) -> GameSeat {
        let index = self.offset + u64::from(i);
        let a_player = if index % 2 == 0 {
            Player::P1
        } else {
            Player::P2
        };
        let first_player = match self.seating {
            Seating::AlternateFirst => Player::P1,
            Seating::AFirstAlternateColors => a_player,
        };
        GameSeat {
            index,
            seed: derive_seed(self.master_seed, index),
            a_player,
            first_player,
        }
    }

    /// Placements de toutes les parties du plan.
    pub fn seats(&self) -> impl Iterator<Item = GameSeat> + '_ {
        (0..self.n_games).map(|i| self.seat(i))
    }
}

/// Générateur propre à un joueur dans une partie : graine et flux distincts
/// pour chaque joueur, aucun état partagé.
#[must_use]
pub fn player_rng(game_seed: u64, player: Player) -> Pcg32 {
    let stream = 1 + player.index() as u64;
    Pcg32::new(derive_seed(game_seed, stream), stream)
}

/// Applique `job` à `0..n` sur `threads` threads et renvoie les résultats dans
/// l'ordre des indices : le résultat ne dépend pas du nombre de threads.
pub fn parallel_map<T, F>(n: usize, threads: usize, job: F) -> Vec<T>
where
    T: Send,
    F: Fn(usize) -> T + Sync,
{
    if threads <= 1 || n <= 1 {
        return (0..n).map(&job).collect();
    }
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<T>>> = (0..n).map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads.min(n) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let value = job(i);
                    *slots[i]
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(value);
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .expect("chaque indice est traité exactement une fois")
        })
        .collect()
}
