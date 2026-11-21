//! Observateurs de partie : découplent l'orchestration des
//! effets de bord (enregistrement, journal, progression, affichage).

use std::time::{Duration, Instant};

use game_core::{Game, Outcome, Player};

use crate::report::SearchReport;

/// Observateur d'une partie.
pub trait MatchObserver<G: Game> {
    /// Début de partie.
    fn on_start(&mut self, _initial: &G) {}

    /// Coup joué (`ply` = numéro du coup, à partir de 1).
    fn on_move(&mut self, ply: u32, player: Player, mv: G::Move, report: &SearchReport<G::Move>);

    /// Fin de partie.
    fn on_end(&mut self, outcome: Outcome, final_state: &G);
}

/// Observateur vide (Null Object).
#[derive(Clone, Copy, Debug, Default)]
pub struct NullObserver;

impl<G: Game> MatchObserver<G> for NullObserver {
    fn on_move(&mut self, _: u32, _: Player, _: G::Move, _: &SearchReport<G::Move>) {}
    fn on_end(&mut self, _: Outcome, _: &G) {}
}

/// Coup enregistré.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveRecord<M> {
    /// Numéro du coup (à partir de 1).
    pub ply: u32,
    /// Joueur qui a joué.
    pub player: Player,
    /// Coup.
    pub mv: M,
    /// Rapport de recherche, si demandé.
    pub report: Option<SearchReport<M>>,
}

/// Partie enregistrée : suffisante pour la rejouer.
#[derive(Clone, Debug, PartialEq)]
pub struct GameRecord<M> {
    /// Joueur qui a commencé.
    pub first_player: Option<Player>,
    /// Coups dans l'ordre.
    pub moves: Vec<MoveRecord<M>>,
    /// Résultat.
    pub outcome: Option<Outcome>,
    /// Durée de la partie (non déterministe).
    pub duration: Duration,
}

/// Observateur qui enregistre la partie.
#[derive(Debug)]
pub struct RecordingObserver<M> {
    record: GameRecord<M>,
    keep_reports: bool,
    started: Option<Instant>,
}

impl<M> RecordingObserver<M> {
    /// Enregistreur ; `keep_reports` conserve le rapport de chaque coup.
    #[must_use]
    pub fn new(keep_reports: bool) -> Self {
        RecordingObserver {
            record: GameRecord {
                first_player: None,
                moves: Vec::new(),
                outcome: None,
                duration: Duration::ZERO,
            },
            keep_reports,
            started: None,
        }
    }

    /// Partie enregistrée.
    #[must_use]
    pub fn into_record(self) -> GameRecord<M> {
        self.record
    }
}

impl<G: Game> MatchObserver<G> for RecordingObserver<G::Move> {
    fn on_start(&mut self, initial: &G) {
        self.record.first_player = Some(initial.current_player());
        self.started = Some(Instant::now());
    }

    fn on_move(&mut self, ply: u32, player: Player, mv: G::Move, report: &SearchReport<G::Move>) {
        self.record.moves.push(MoveRecord {
            ply,
            player,
            mv,
            report: self.keep_reports.then(|| report.clone()),
        });
    }

    fn on_end(&mut self, outcome: Outcome, _final_state: &G) {
        self.record.outcome = Some(outcome);
        if let Some(t) = self.started {
            self.record.duration = t.elapsed();
        }
    }
}

/// Diffuse les événements à plusieurs observateurs.
pub struct CompositeObserver<'a, G: Game> {
    observers: Vec<&'a mut dyn MatchObserver<G>>,
}

impl<'a, G: Game> CompositeObserver<'a, G> {
    /// Composite à partir d'une liste d'observateurs.
    #[must_use]
    pub fn new(observers: Vec<&'a mut dyn MatchObserver<G>>) -> Self {
        CompositeObserver { observers }
    }
}

impl<G: Game> MatchObserver<G> for CompositeObserver<'_, G> {
    fn on_start(&mut self, initial: &G) {
        for o in &mut self.observers {
            o.on_start(initial);
        }
    }

    fn on_move(&mut self, ply: u32, player: Player, mv: G::Move, report: &SearchReport<G::Move>) {
        for o in &mut self.observers {
            o.on_move(ply, player, mv, report);
        }
    }

    fn on_end(&mut self, outcome: Outcome, final_state: &G) {
        for o in &mut self.observers {
            o.on_end(outcome, final_state);
        }
    }
}
