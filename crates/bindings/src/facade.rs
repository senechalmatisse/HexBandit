//! Façade : `play_game`, `play_batch`, `search`. Entrées et
//! sorties en JSON ; les enregistrements suivent le format versionné `hexbandit/game-record`.

use game_core::{Outcome, Player};
use mcts::{
    CancelToken, ReportDetail, SearchReport,
    observer::{GameRecord, MatchObserver, RecordingObserver},
    runner::{parallel_map, play_game as run_game},
};

use crate::{
    config::{GameConfig, GameSpec, MatchSpec, ReportLevel, SearchRequest, color_name},
    error::{BindingError, ErrorKind},
    json::Json,
    log,
    registry::{EngineGame, GameVisitor, build_agent, dispatch},
};

/// Nom du format d'enregistrement.
pub const RECORD_FORMAT: &str = "hexbandit/game-record";
/// Version du format d'enregistrement.
pub const RECORD_FORMAT_VERSION: u64 = 1;
/// Version du moteur.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

type Result<T> = std::result::Result<T, BindingError>;

fn detail_for(level: ReportLevel) -> ReportDetail {
    match level {
        ReportLevel::Full => ReportDetail::Full,
        _ => ReportDetail::Summary,
    }
}

fn report_json<G: EngineGame>(game: &G, report: &SearchReport<G::Move>, level: ReportLevel) -> Json
where
    G::Move: Send + Sync,
{
    let mut fields = vec![
        ("simulations", Json::int(report.simulations)),
        ("nodes", Json::int(report.nodes)),
        ("max_depth", Json::int(report.max_depth)),
        ("mean_depth", Json::float32(report.mean_depth)),
        ("root_value", Json::float32(report.root_value)),
        ("best_visits", Json::int(report.best_visits)),
        ("best_value", Json::float32(report.best_value)),
        ("node_limit_reached", Json::Bool(report.node_limit_reached)),
        ("elapsed_ms", Json::float(report.elapsed_us as f64 / 1000.0)),
    ];
    if report.reused_visits > 0 {
        fields.push(("reused_visits", Json::int(report.reused_visits)));
    }
    if let Some(v) = report.proven {
        fields.push(("proven", Json::str(proven_name(v))));
    }
    if level == ReportLevel::Full {
        let candidates = report
            .candidates
            .iter()
            .map(|c| {
                let mut fields = vec![
                    ("move", Json::str(game.format_move(c.mv))),
                    ("n", Json::int(c.visits)),
                    ("q", Json::float32(c.q)),
                    ("n_amaf", Json::int(c.amaf_visits)),
                    ("q_amaf", Json::float32(c.amaf_q)),
                    ("value", Json::float32(c.value)),
                ];
                if let Some(v) = c.proven {
                    fields.push(("proven", Json::str(proven_name(v))));
                }
                Json::object(fields)
            })
            .collect();
        fields.push(("candidates", Json::Array(candidates)));
    }
    Json::object(fields)
}

/// Nom d'une valeur prouvée par le solveur (demi-points du joueur concerné).
fn proven_name(value: u8) -> &'static str {
    match value {
        0 => "loss",
        1 => "draw",
        _ => "win",
    }
}

fn outcome_json(outcome: Option<Outcome>) -> Json {
    match outcome {
        Some(Outcome::Win(p)) => Json::str(color_name(p)),
        Some(Outcome::Draw) => Json::str("draw"),
        None => Json::Null,
    }
}

fn record_json<G: EngineGame>(
    config: &GameConfig,
    initial: &G,
    record: &GameRecord<G::Move>,
) -> Json
where
    G::Move: Send + Sync,
{
    let moves = record
        .moves
        .iter()
        .map(|m| {
            let mut fields = vec![
                ("ply", Json::int(m.ply)),
                ("player", Json::str(color_name(m.player))),
                ("move", Json::str(initial.format_move(m.mv))),
            ];
            if let Some(report) = &m.report {
                fields.push(("report", report_json(initial, report, config.report)));
            }
            Json::object(fields)
        })
        .collect();
    let mut fields = vec![
        ("format", Json::str(RECORD_FORMAT)),
        ("format_version", Json::int(RECORD_FORMAT_VERSION)),
        (
            "engine",
            Json::object(vec![
                ("name", Json::str("hexbandit-native")),
                ("version", Json::str(ENGINE_VERSION)),
            ]),
        ),
        ("game", config.game.to_json()),
        ("first_player", Json::str(color_name(config.first_player))),
        ("seed", Json::int(config.seed)),
        (
            "players",
            Json::object(vec![
                ("blue", config.players[0].to_json()),
                ("red", config.players[1].to_json()),
            ]),
        ),
        (
            "opening",
            Json::Array(config.opening.iter().map(Json::str).collect()),
        ),
        ("report_level", Json::str(config.report.as_str())),
        ("moves", Json::Array(moves)),
        ("num_moves", Json::int(record.moves.len() as u64)),
        ("winner", outcome_json(record.outcome)),
        (
            "duration_ms",
            Json::float(record.duration.as_secs_f64() * 1000.0),
        ),
    ];
    if let Some(info) = config.match_info {
        fields.push((
            "match",
            Json::object(vec![
                ("index", Json::int(info.index)),
                ("a_color", Json::str(color_name(info.a_color))),
            ]),
        ));
    }
    Json::object(fields)
}

/// Exécute une partie configurée (générique sur le jeu concret).
struct PlayOne<'a> {
    config: &'a GameConfig,
    cancel: &'a CancelToken,
}

impl GameVisitor for PlayOne<'_> {
    type Output = Result<Json>;

    fn visit<G: EngineGame>(self, spec: &GameSpec) -> Self::Output
    where
        G::Move: Send + Sync,
    {
        let config = self.config;
        let mut initial = G::initial(spec, config.first_player)?;
        for text in &config.opening {
            let mv = initial.parse_move(text)?;
            initial.play(mv)?;
        }
        let detail = detail_for(config.report);
        let mut blue = build_agent::<G>(&config.players[0], config.seed, Player::P1, detail)?;
        let mut red = build_agent::<G>(&config.players[1], config.seed, Player::P2, detail)?;
        let mut recorder: RecordingObserver<G::Move> =
            RecordingObserver::new(config.report != ReportLevel::None);
        let observer: &mut dyn MatchObserver<G> = &mut recorder;
        run_game(
            initial.clone(),
            [blue.as_mut(), red.as_mut()],
            observer,
            self.cancel,
            config.max_plies,
        )?;
        let record = recorder.into_record();
        Ok(record_json(config, &initial, &record))
    }
}

fn play_config(config: &GameConfig, cancel: &CancelToken) -> Result<Json> {
    dispatch(&config.game, PlayOne { config, cancel })
}

/// Joue une partie ; renvoie l'enregistrement JSON.
///
/// # Errors
///
/// Configuration invalide, coup d'ouverture illégal ou annulation.
pub fn play_game(request: &str, cancel: &CancelToken) -> Result<Json> {
    let value = Json::parse(request)?;
    let config = GameConfig::parse(&value, "")?;
    log::info(&format!(
        "partie {}x{} : {} (bleu) contre {} (rouge)",
        config.game.size,
        config.game.size,
        config.players[0].short_label(),
        config.players[1].short_label()
    ));
    play_config(&config, cancel)
}

/// Joue un lot de parties en parallèle. La requête contient soit
/// `games` (liste de configurations), soit `match` (match A contre B). Les
/// enregistrements sont renvoyés dans l'ordre, indépendamment de `threads`.
///
/// # Errors
///
/// Première erreur rencontrée (dans l'ordre des parties), ou annulation.
pub fn play_batch(request: &str, threads: usize, cancel: &CancelToken) -> Result<Json> {
    let value = Json::parse(request)?;
    let mut f = crate::config::Fields::new(&value, "")?;
    let version = f
        .opt_u64("schema_version")?
        .unwrap_or(crate::config::SCHEMA_VERSION);
    if version != crate::config::SCHEMA_VERSION {
        return Err(BindingError::config(format!(
            "schema_version {version} non pris en charge"
        )));
    }
    let games_value = f.take("games");
    let match_value = f.take("match");
    f.finish()?;
    let configs = match (games_value, match_value) {
        (Some(Json::Array(items)), None) => items
            .iter()
            .enumerate()
            .map(|(i, item)| GameConfig::parse(item, &format!("games[{i}]")))
            .collect::<Result<Vec<_>>>()?,
        (Some(other), None) => {
            return Err(BindingError::config(format!(
                "games : tableau attendu, {} reçu",
                other.type_name()
            )));
        }
        (None, Some(m)) => MatchSpec::parse(m, "match")?.games(),
        _ => {
            return Err(BindingError::config(
                "préciser exactement un champ parmi games et match",
            ));
        }
    };
    log::info(&format!(
        "lot de {} parties sur {} thread(s)",
        configs.len(),
        threads.max(1)
    ));
    let results = parallel_map(configs.len(), threads.max(1), |i| {
        play_config(&configs[i], cancel)
    });
    let mut records = Vec::with_capacity(results.len());
    for result in results {
        records.push(result?);
    }
    Ok(Json::object(vec![("records", Json::Array(records))]))
}

struct SearchOne<'a> {
    request: &'a SearchRequest,
    cancel: &'a CancelToken,
}

impl GameVisitor for SearchOne<'_> {
    type Output = Result<Json>;

    fn visit<G: EngineGame>(self, spec: &GameSpec) -> Self::Output
    where
        G::Move: Send + Sync,
    {
        let request = self.request;
        let state = match &request.position {
            Some(text) => G::from_position(spec, text)?,
            None => {
                let mut state = G::initial(spec, request.first_player)?;
                for text in &request.moves {
                    let mv = state.parse_move(text)?;
                    state.play(mv)?;
                }
                state
            }
        };
        let color = state.current_player();
        let mut agent = build_agent::<G>(&request.player, request.seed, color, ReportDetail::Full)?;
        let report = agent.choose(&state, self.cancel)?;
        Ok(Json::object(vec![
            ("position", Json::str(state.position())),
            ("player", Json::str(color_name(color))),
            ("best_move", Json::str(state.format_move(report.best_move))),
            ("player_config", request.player.to_json()),
            ("report", report_json(&state, &report, ReportLevel::Full)),
        ]))
    }
}

/// Recherche sur une position (carte de chaleur, analyse d'un coup).
///
/// # Errors
///
/// Requête invalide, position terminale ou annulation.
pub fn search(request: &str, cancel: &CancelToken) -> Result<Json> {
    let value = Json::parse(request)?;
    let request = SearchRequest::parse(&value)?;
    dispatch(
        &request.game,
        SearchOne {
            request: &request,
            cancel,
        },
    )
}

/// Informations de version.
#[must_use]
pub fn version() -> Json {
    Json::object(vec![
        ("engine_version", Json::str(ENGINE_VERSION)),
        ("record_format", Json::str(RECORD_FORMAT)),
        ("record_format_version", Json::int(RECORD_FORMAT_VERSION)),
        (
            "config_schema_version",
            Json::int(crate::config::SCHEMA_VERSION),
        ),
        (
            "default_rave_bias",
            Json::float32(crate::config::DEFAULT_RAVE_BIAS),
        ),
        (
            "default_hand_k",
            Json::float32(crate::config::DEFAULT_HAND_K),
        ),
        (
            "default_max_nodes",
            Json::int(crate::config::DEFAULT_MAX_NODES),
        ),
    ])
}

/// Enveloppe de réponse : `{"ok": true, "result": ...}` ou
/// `{"ok": false, "error": {"kind": ..., "message": ...}}`.
#[must_use]
pub fn envelope(result: Result<Json>) -> String {
    match result {
        Ok(value) => Json::object(vec![("ok", Json::Bool(true)), ("result", value)]).to_string(),
        Err(error) => {
            if error.kind != ErrorKind::Cancelled {
                log::warn(&error.to_string());
            }
            Json::object(vec![("ok", Json::Bool(false)), ("error", error.to_json())]).to_string()
        }
    }
}
