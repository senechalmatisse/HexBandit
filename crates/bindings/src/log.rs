//! Journal minimal sur la sortie d'erreur.
//!
//! Le niveau se règle avec la variable d'environnement `HEXBANDIT_LOG`
//! (`off`, `error`, `warn` par défaut, `info`, `debug`).
//! Seul ce crate (frontière d'entrée-sortie) écrit dans le journal.

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Off,
    Error,
    Warn,
    Info,
    Debug,
}

fn level() -> Level {
    static LEVEL: OnceLock<Level> = OnceLock::new();
    *LEVEL.get_or_init(|| match std::env::var("HEXBANDIT_LOG").as_deref() {
        Ok("off") => Level::Off,
        Ok("error") => Level::Error,
        Ok("info") => Level::Info,
        Ok("debug") => Level::Debug,
        _ => Level::Warn,
    })
}

fn emit(lvl: Level, tag: &str, message: &str) {
    if lvl <= level() && lvl != Level::Off {
        eprintln!("[hexbandit-native {tag}] {message}");
    }
}

/// Message d'avertissement.
pub(crate) fn warn(message: &str) {
    emit(Level::Warn, "WARN", message);
}

/// Message d'information.
pub(crate) fn info(message: &str) {
    emit(Level::Info, "INFO", message);
}

/// Message de mise au point.
#[allow(dead_code)]
pub(crate) fn debug(message: &str) {
    emit(Level::Debug, "DEBUG", message);
}

/// Message d'erreur.
pub(crate) fn error(message: &str) {
    emit(Level::Error, "ERROR", message);
}
