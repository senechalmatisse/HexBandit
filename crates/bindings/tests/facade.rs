//! Tests de la façade : format d'enregistrement, déterminisme, validation,
//! parallélisme, variantes et appel par l'ABI C.

use std::ffi::{CStr, CString};

use hexbandit_native::{
    error::ErrorKind,
    facade::{self, ENGINE_VERSION, RECORD_FORMAT},
    ffi,
    json::Json,
};
use mcts::CancelToken;

fn game_request(size: u64, blue: &str, red: &str, seed: u64, report: &str) -> String {
    format!(
        r#"{{"schema_version":1,"game":{{"kind":"hex","size":{size}}},
            "players":{{"blue":{blue},"red":{red}}},"seed":{seed},"report":"{report}"}}"#
    )
}

const UCT: &str = r#"{"algorithm":"uct","simulations":200}"#;
const RAVE: &str = r#"{"algorithm":"rave","simulations":200}"#;

fn moves(record: &Json) -> Vec<String> {
    match record.get("moves") {
        Some(Json::Array(items)) => items
            .iter()
            .map(|m| match m.get("move") {
                Some(Json::String(s)) => s.clone(),
                _ => panic!("coup sans notation"),
            })
            .collect(),
        _ => panic!("pas de coups"),
    }
}

#[test]
fn record_format_and_determinism() {
    let request = game_request(6, UCT, RAVE, 42, "full");
    let a = facade::play_game(&request, &CancelToken::new()).unwrap();
    let b = facade::play_game(&request, &CancelToken::new()).unwrap();
    assert_eq!(moves(&a), moves(&b));
    assert_eq!(a.get("format"), Some(&Json::str(RECORD_FORMAT)));
    assert_eq!(a.get("format_version"), Some(&Json::int(1u64)));
    let engine = a.get("engine").unwrap();
    assert_eq!(engine.get("version"), Some(&Json::str(ENGINE_VERSION)));
    assert!(matches!(a.get("winner"), Some(Json::String(w)) if w == "blue" || w == "red"));
    let first = match a.get("moves") {
        Some(Json::Array(items)) => items[0].clone(),
        _ => unreachable!(),
    };
    assert_eq!(first.get("ply"), Some(&Json::int(1u64)));
    assert_eq!(first.get("player"), Some(&Json::str("blue")));
    let report = first.get("report").unwrap();
    assert_eq!(report.get("simulations"), Some(&Json::int(200u64)));
    assert!(matches!(report.get("candidates"), Some(Json::Array(c)) if c.len() == 36));
    // Les joueurs sont renvoyés normalisés (valeurs par défaut explicites).
    let red = a.get("players").unwrap().get("red").unwrap();
    assert!(red.get("beta").is_some());
    let c =
        facade::play_game(&game_request(6, UCT, RAVE, 43, "none"), &CancelToken::new()).unwrap();
    assert_ne!(moves(&a), moves(&c));
}

#[test]
fn validation_errors_are_typed() {
    let cases = [
        (r#"{"players":{}}"#, ErrorKind::Config),
        (r#"not json"#, ErrorKind::Config),
        (
            &game_request(25, UCT, UCT, 1, "none") as &str,
            ErrorKind::Config,
        ),
        (
            &game_request(
                5,
                UCT,
                r#"{"algorithm":"uct","simulations":10,"oops":1}"#,
                1,
                "none",
            ),
            ErrorKind::Config,
        ),
        (
            r#"{"game":{"size":5},"players":{"blue":{"algorithm":"uct","simulations":10},"red":{"algorithm":"uct","simulations":10}},"opening":["a1","a1"]}"#,
            ErrorKind::Game,
        ),
        (
            r#"{"schema_version":2,"players":{"blue":{"algorithm":"uct","simulations":10},"red":{"algorithm":"uct","simulations":10}}}"#,
            ErrorKind::Config,
        ),
    ];
    for (request, kind) in cases {
        let err = facade::play_game(request, &CancelToken::new()).unwrap_err();
        assert_eq!(err.kind, kind, "{request} -> {err}");
    }
    let cancel = CancelToken::new();
    cancel.cancel();
    let err = facade::play_game(&game_request(5, UCT, UCT, 1, "none"), &cancel).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Cancelled);
}

#[test]
fn batch_is_independent_of_thread_count() {
    let request = r#"{"schema_version":1,"match":{"game":{"size":5},
        "a":{"algorithm":"uct","simulations":150},"b":{"algorithm":"rave","simulations":50},
        "n_games":6,"master_seed":7,"seating":"a_first_alternate_colors"}}"#;
    let one = facade::play_batch(request, 1, &CancelToken::new()).unwrap();
    let two = facade::play_batch(request, 2, &CancelToken::new()).unwrap();
    let records = |v: &Json| match v.get("records") {
        Some(Json::Array(items)) => items.iter().map(moves).collect::<Vec<_>>(),
        _ => panic!(),
    };
    assert_eq!(records(&one), records(&two));
    let Some(Json::Array(items)) = one.get("records") else {
        panic!()
    };
    for (i, record) in items.iter().enumerate() {
        let info = record.get("match").unwrap();
        assert_eq!(info.get("index"), Some(&Json::int(i as u64)));
        let a_color = info.get("a_color").unwrap();
        // A joue en premier, sa couleur alterne.
        assert_eq!(record.get("first_player"), Some(a_color));
        assert_eq!(a_color, &Json::str(if i % 2 == 0 { "blue" } else { "red" }));
    }
    // Une tranche du match redonne les mêmes parties.
    let slice = request.replace(r#""n_games":6"#, r#""n_games":2,"offset":4"#);
    let part = facade::play_batch(&slice, 1, &CancelToken::new()).unwrap();
    assert_eq!(records(&part), records(&one)[4..].to_vec());
}

#[test]
fn variants_and_search() {
    for game in [
        r#"{"kind":"reverse_hex","size":5}"#,
        r#"{"kind":"hex","size":5,"swap":true}"#,
        r#"{"kind":"reverse_hex","size":5,"swap":true}"#,
    ] {
        let request = format!(
            r#"{{"game":{game},"players":{{"blue":{RAVE},"red":{UCT}}},"seed":3,"report":"summary"}}"#
        );
        let record = facade::play_game(&request, &CancelToken::new()).unwrap();
        assert!(!moves(&record).is_empty(), "{game}");
    }
    // Recherche : coup gagnant unique (voir tests du moteur).
    let request = r#"{"game":{"size":5},"moves":["a1","c2","b1","c3","d1","c4","e1","c5"],
        "player":{"algorithm":"rave","simulations":2000},"seed":1}"#;
    let result = facade::search(request, &CancelToken::new()).unwrap();
    assert_eq!(result.get("best_move"), Some(&Json::str("c1")));
    assert_eq!(result.get("player"), Some(&Json::str("blue")));
    let by_position = r#"{"game":{"size":5},"position":"b.bbb/..r../..r../..r../..r.. b",
        "player":{"algorithm":"uct","simulations":2000},"seed":1}"#;
    let result = facade::search(by_position, &CancelToken::new()).unwrap();
    assert_eq!(result.get("best_move"), Some(&Json::str("b1")));
}

#[test]
fn c_abi_round_trip() {
    let call = |f: unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_char,
                input: &str| {
        let c_input = CString::new(input).unwrap();
        // SAFETY : chaîne valide ; le résultat est libéré par hexbandit_free_string.
        unsafe {
            let out = f(c_input.as_ptr());
            let text = CStr::from_ptr(out).to_str().unwrap().to_owned();
            ffi::hexbandit_free_string(out);
            Json::parse(&text).unwrap()
        }
    };
    let ok = call(
        ffi::hexbandit_play_game,
        &game_request(4, UCT, UCT, 1, "none"),
    );
    assert_eq!(ok.get("ok"), Some(&Json::Bool(true)));
    let err = call(ffi::hexbandit_play_game, "{");
    assert_eq!(err.get("ok"), Some(&Json::Bool(false)));
    assert_eq!(
        err.get("error").unwrap().get("kind"),
        Some(&Json::str("config"))
    );
    // SAFETY : un pointeur nul est accepté et signalé.
    let null = unsafe { ffi::hexbandit_search(std::ptr::null()) };
    // SAFETY : chaîne renvoyée par la bibliothèque.
    let text = unsafe { CStr::from_ptr(null).to_str().unwrap().to_owned() };
    // SAFETY : libération unique.
    unsafe { ffi::hexbandit_free_string(null) };
    assert!(text.contains("pointeur nul"));
    let version = ffi::hexbandit_version();
    // SAFETY : chaîne renvoyée par la bibliothèque, libérée une fois.
    unsafe { ffi::hexbandit_free_string(version) };
}

#[test]
fn quarto_through_the_registry() {
    let request = r#"{"game":{"kind":"quarto"},"players":{"blue":{"algorithm":"uct","simulations":300},
        "red":{"algorithm":"random"}},"seed":5,"report":"summary"}"#;
    let record = facade::play_game(request, &CancelToken::new()).unwrap();
    let first = moves(&record)[0].clone();
    assert!(
        first.starts_with('/'),
        "le premier coup offre une pièce : {first}"
    );
    assert!(
        matches!(record.get("winner"), Some(Json::String(w)) if ["blue", "red", "draw"].contains(&w.as_str()))
    );
    // MC-RAVE est refusé : le Quarto n'implémente pas AmafGame.
    let rave = request.replace(
        r#"{"algorithm":"uct","simulations":300}"#,
        r#"{"algorithm":"rave","simulations":300}"#,
    );
    let err = facade::play_game(&rave, &CancelToken::new()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Config);
    assert!(err.message.contains("AmafGame"));
    let bad_size = request.replace(r#""kind":"quarto""#, r#""kind":"quarto","size":5"#);
    assert_eq!(
        facade::play_game(&bad_size, &CancelToken::new())
            .unwrap_err()
            .kind,
        ErrorKind::Config
    );
    // Des matchs nuls sont possibles entre joueurs de même force.
    let batch = r#"{"match":{"game":{"kind":"quarto"},"a":{"algorithm":"uct","simulations":400},
        "b":{"algorithm":"uct","simulations":400},"n_games":12,"master_seed":3}}"#;
    let result = facade::play_batch(batch, 2, &CancelToken::new()).unwrap();
    let Some(Json::Array(records)) = result.get("records") else {
        panic!()
    };
    assert_eq!(records.len(), 12);
}

fn first_report(record: &Json, ply: usize) -> Json {
    match record.get("moves") {
        Some(Json::Array(items)) => items[ply].get("report").unwrap().clone(),
        _ => panic!("pas de coups"),
    }
}

#[test]
fn v2_options_through_the_facade() {
    // Les options par défaut ne changent pas l'enregistrement.
    let plain =
        facade::play_game(&game_request(5, UCT, RAVE, 9, "full"), &CancelToken::new()).unwrap();
    let text = plain.to_string();
    for key in [
        "reused_visits",
        "proven",
        "solver",
        "reuse_tree",
        "grave_ref",
        "trees",
    ] {
        assert!(!text.contains(key), "{key} dans un enregistrement V1");
    }
    // Solveur, réutilisation, GRAVE, arbres parallèles et a priori du Hex.
    let v2 = r#"{"algorithm":"rave","simulations":400,"solver":true,"reuse_tree":true,
        "grave_ref":30,"trees":2,"prior":{"kind":"hex"},"final_move":"secure"}"#;
    let record =
        facade::play_game(&game_request(5, v2, UCT, 4, "full"), &CancelToken::new()).unwrap();
    let blue = record.get("players").unwrap().get("blue").unwrap();
    assert_eq!(blue.get("trees"), Some(&Json::int(2u64)));
    assert_eq!(
        blue.get("prior").unwrap().get("kind"),
        Some(&Json::str("hex"))
    );
    let again =
        facade::play_game(&game_request(5, v2, UCT, 4, "full"), &CancelToken::new()).unwrap();
    assert_eq!(
        moves(&record),
        moves(&again),
        "déterministe malgré les threads"
    );
    let text = record.to_string();
    assert!(
        text.contains("\"reused_visits\""),
        "réutilisation visible dans les rapports"
    );
    assert!(
        text.contains("\"proven\":\"win\""),
        "fin de partie résolue par le solveur"
    );
    // Le troisième coup de Bleu a hérité d'une partie de l'arbre.
    let report = first_report(&record, 2);
    assert_eq!(report.get("simulations"), Some(&Json::int(400u64)));
    // A priori du Hex refusé pour Reverse Hex ; GRAVE refusé pour UCT.
    let reverse = format!(
        r#"{{"game":{{"kind":"reverse_hex","size":5}},"players":{{"blue":{v2},"red":{UCT}}}}}"#
    );
    let err = facade::play_game(&reverse, &CancelToken::new()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Config);
    let bad = r#"{"algorithm":"uct","simulations":10,"grave_ref":5}"#;
    assert_eq!(
        facade::play_game(&game_request(5, bad, UCT, 1, "none"), &CancelToken::new())
            .unwrap_err()
            .kind,
        ErrorKind::Config
    );
    // Règle du gâteau : l'a priori du Hex s'applique aux coups ordinaires.
    let swap = format!(
        r#"{{"game":{{"kind":"hex","size":5,"swap":true}},"players":{{"blue":{v2},"red":{RAVE}}},"seed":2}}"#
    );
    assert!(!moves(&facade::play_game(&swap, &CancelToken::new()).unwrap()).is_empty());
    // Recherche : position résolue par le solveur.
    let request = r#"{"game":{"size":5},"moves":["a1","c2","b1","c3","d1","c4","e1","c5"],
        "player":{"algorithm":"uct","simulations":3000,"solver":true,"exploration":0.25},"seed":1}"#;
    let result = facade::search(request, &CancelToken::new()).unwrap();
    assert_eq!(result.get("best_move"), Some(&Json::str("c1")));
    assert_eq!(
        result.get("report").unwrap().get("proven"),
        Some(&Json::str("win"))
    );
}

#[test]
fn chameleon_through_the_registry() {
    // UCT et MC-RAVE (AmafGame : case × 2 + couleur), avec et sans gâteau.
    for (game, blue) in [
        (r#"{"kind":"chameleon","size":5}"#, RAVE),
        (r#"{"kind":"chameleon","size":5,"swap":true}"#, RAVE),
        (r#"{"kind":"chameleon","size":4}"#, UCT),
    ] {
        let request = format!(
            r#"{{"game":{game},"players":{{"blue":{blue},"red":{UCT}}},"seed":5,"report":"full"}}"#
        );
        let record = facade::play_game(&request, &CancelToken::new()).unwrap();
        let played = moves(&record);
        assert!(!played.is_empty(), "{game}");
        assert!(
            played
                .iter()
                .all(|m| m == "swap" || m.ends_with('b') || m.ends_with('r')),
            "{played:?}"
        );
        assert!(matches!(record.get("winner"), Some(Json::String(w)) if w == "blue" || w == "red"));
        let first = first_report(&record, 0);
        let Some(Json::Array(candidates)) = first.get("candidates") else {
            panic!("pas de candidats")
        };
        let size: usize = if game.contains("\"size\":5") { 5 } else { 4 };
        assert_eq!(candidates.len(), 2 * size * size);
    }
    // Recherche sur une position textuelle : Bleu (Horz) gagne en c2 rouge
    // (chaîne rouge a2-b2-c2 d'Ouest en Est sur 3 × 3).
    let request = r#"{"game":{"kind":"chameleon","size":3},"position":"b../rr./..b b",
        "player":{"algorithm":"rave","simulations":500},"seed":1}"#;
    let result = facade::search(request, &CancelToken::new()).unwrap();
    assert_eq!(result.get("best_move"), Some(&Json::str("c2r")));
    // Le prior du Hex ne s'applique pas à Chameleon.
    let bad = r#"{"game":{"kind":"chameleon","size":4},"players":{"blue":{"algorithm":"rave","simulations":10,"prior":{"kind":"hex"}},"red":{"algorithm":"uct","simulations":10}}}"#;
    assert_eq!(
        facade::play_game(bad, &CancelToken::new())
            .unwrap_err()
            .kind,
        ErrorKind::Config
    );
}
