//! Tests de contrat génériques.
//!
//! Ces fonctions vérifient, par des parties aléatoires, que toute
//! implémentation de [`Game`], [`AmafGame`] ou [`RolloutPolicy`] respecte le
//! contrat documenté de son interface. Elles sont publiques pour que chaque
//! crate de jeu puisse les exécuter sur ses propres types.

use crate::{
    AmafGame, Game, GameError,
    rng::{Pcg32, Rng},
    rollout::{RolloutPolicy, Trace, TraceMode},
};

fn fail<T>(message: String) -> Result<T, String> {
    Err(message)
}

/// Vérifie le contrat de [`Game`] sur `games` parties aléatoires partant de
/// `initial`, chacune limitée à `max_plies` coups.
///
/// # Errors
///
/// Renvoie une description de la première violation rencontrée.
pub fn check_game_contract<G: Game>(
    initial: &G,
    seed: u64,
    games: usize,
    max_plies: usize,
) -> Result<(), String> {
    let mut rng = Pcg32::new(seed, 0xC0);
    let mut moves = Vec::new();
    let mut buf = Vec::new();
    for game_index in 0..games {
        let mut state = initial.clone();
        let mut history = Vec::new();
        for _ in 0..=max_plies {
            state.legal_moves(&mut moves);
            let terminal = state.outcome().is_some();
            if terminal != moves.is_empty() {
                return fail(format!(
                    "partie {game_index} : terminale={terminal} mais {} coups légaux",
                    moves.len()
                ));
            }
            if terminal {
                let before = state.clone();
                let ply = state.ply();
                let probe = history.last().copied();
                if let Some(mv) = probe {
                    match state.play(mv) {
                        Err(GameError::GameOver) => {}
                        other => {
                            return fail(format!(
                                "partie {game_index} : jouer après la fin a renvoyé {other:?}"
                            ));
                        }
                    }
                }
                if state.ply() != ply || state.outcome() != before.outcome() {
                    return fail("une erreur a modifié l'état".to_owned());
                }
                if state.random_legal_move(&mut rng, &mut buf).is_some() {
                    return fail("random_legal_move sur une position terminale".to_owned());
                }
                break;
            }
            // random_legal_move doit renvoyer un coup légal.
            let random = state.random_legal_move(&mut rng, &mut buf);
            match random {
                Some(mv) if moves.contains(&mv) => {}
                other => {
                    return fail(format!(
                        "partie {game_index} : random_legal_move a renvoyé {other:?}"
                    ));
                }
            }
            // Chaque coup légal doit être jouable sur une copie.
            for &mv in &moves {
                let mut copy = state.clone();
                if let Err(e) = copy.play(mv) {
                    return fail(format!("coup légal {mv:?} refusé : {e}"));
                }
                if copy.ply() != state.ply() + 1 {
                    return fail("ply n'a pas augmenté de un".to_owned());
                }
            }
            // clone_from doit reproduire l'état.
            let mut other = initial.clone();
            other.clone_from(&state);
            let mut other_moves = Vec::new();
            other.legal_moves(&mut other_moves);
            if other_moves != moves
                || other.current_player() != state.current_player()
                || other.ply() != state.ply()
            {
                return fail("clone_from ne reproduit pas l'état".to_owned());
            }
            let mv = moves[rng.below(moves.len() as u32) as usize];
            if let Err(e) = state.play(mv) {
                return fail(format!("coup légal {mv:?} refusé : {e}"));
            }
            history.push(mv);
        }
        if state.outcome().is_none() {
            return fail(format!(
                "partie {game_index} non terminée après {max_plies} coups"
            ));
        }
        // Rejouer l'historique depuis le début doit redonner le même résultat.
        let mut replay = initial.clone();
        for &mv in &history {
            if let Err(e) = replay.play(mv) {
                return fail(format!("rejeu impossible : {e}"));
            }
        }
        if replay.outcome() != state.outcome() || replay.ply() != state.ply() {
            return fail("le rejeu ne redonne pas le même résultat".to_owned());
        }
    }
    Ok(())
}

/// Vérifie le contrat de [`AmafGame`] (indices denses, distincts, bornés).
///
/// # Errors
///
/// Renvoie une description de la première violation rencontrée.
pub fn check_amaf_contract<G: AmafGame>(
    initial: &G,
    seed: u64,
    games: usize,
) -> Result<(), String> {
    let mut rng = Pcg32::new(seed, 0xA4);
    let mut moves = Vec::new();
    let bound = initial.move_index_bound();
    for _ in 0..games {
        let mut state = initial.clone();
        while state.outcome().is_none() {
            state.legal_moves(&mut moves);
            let mut indices: Vec<usize> = moves.iter().map(|&m| state.move_index(m)).collect();
            if indices.iter().any(|&i| i >= bound) {
                return fail("indice de coup hors borne".to_owned());
            }
            if state.move_index_bound() != bound {
                return fail("move_index_bound dépend de la position".to_owned());
            }
            for &m in &moves {
                if state.move_index(m) != initial.move_index(m) {
                    return fail("l'indice d'un coup dépend de la position".to_owned());
                }
            }
            indices.sort_unstable();
            indices.dedup();
            if indices.len() != moves.len() {
                return fail("deux coups légaux partagent un indice".to_owned());
            }
            let mv = moves[rng.below(moves.len() as u32) as usize];
            state.play(mv).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Vérifie le contrat d'une [`RolloutPolicy`] : depuis des positions
/// intermédiaires aléatoires, la trace complète rejouée doit être légale et se
/// terminer exactement au dernier coup, avec le même résultat.
///
/// # Errors
///
/// Renvoie une description de la première violation rencontrée.
pub fn check_rollout_contract<G: Game, P: RolloutPolicy<G>>(
    initial: &G,
    policy: &mut P,
    seed: u64,
    games: usize,
) -> Result<(), String> {
    let mut rng = Pcg32::new(seed, 0x50);
    let mut buf = Vec::new();
    let mut trace = Trace::new(TraceMode::Full);
    for game_index in 0..games {
        // Position de départ : quelques coups aléatoires.
        let mut start = initial.clone();
        let prefix = rng.below(12);
        for _ in 0..prefix {
            match start.random_legal_move(&mut rng, &mut buf) {
                Some(mv) => start.play(mv).map_err(|e| e.to_string())?,
                None => break,
            }
        }
        for mode in [TraceMode::Full, TraceMode::FirstMove, TraceMode::Off] {
            trace.set_mode(mode);
            trace.clear();
            let mut scratch = start.clone();
            let outcome = policy.rollout(&mut scratch, &mut rng, &mut trace);
            if start.is_terminal() {
                if Some(outcome) != start.outcome() || !trace.is_empty() {
                    return fail("simulation incohérente depuis une position terminale".to_owned());
                }
                continue;
            }
            match mode {
                TraceMode::Off if !trace.is_empty() => {
                    return fail("trace non vide en mode Off".to_owned());
                }
                TraceMode::FirstMove if trace.len() != 1 => {
                    return fail(format!("{} coups en mode FirstMove", trace.len()));
                }
                TraceMode::Full => {
                    let mut replay = start.clone();
                    let entries = trace.entries();
                    for (i, &(mv, player)) in entries.iter().enumerate() {
                        if replay.is_terminal() {
                            return fail(format!(
                                "partie {game_index} : trace trop longue (fin au coup {i})"
                            ));
                        }
                        if replay.current_player() != player {
                            return fail("joueur incorrect dans la trace".to_owned());
                        }
                        replay
                            .play(mv)
                            .map_err(|e| format!("trace illégale : {e}"))?;
                    }
                    if replay.outcome() != Some(outcome) {
                        return fail(format!(
                            "partie {game_index} : résultat {outcome:?} mais rejeu {:?}",
                            replay.outcome()
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
