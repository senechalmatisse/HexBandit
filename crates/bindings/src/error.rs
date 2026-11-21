//! Erreurs de la façade, converties en exceptions Python typées.

use std::fmt;

use game_core::GameError;
use mcts::SearchError;

use crate::json::{Json, JsonError};

/// Catégorie d'erreur, transmise à Python dans le champ `kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// Configuration ou JSON invalide.
    Config,
    /// Règle du jeu violée (coup d'ouverture illégal, position impossible…).
    Game,
    /// Erreur du moteur de recherche.
    Search,
    /// Calcul interrompu (Ctrl-C).
    Cancelled,
    /// Erreur interne (panique interceptée).
    Internal,
}

impl ErrorKind {
    /// Nom stable de la catégorie.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Config => "config",
            ErrorKind::Game => "game",
            ErrorKind::Search => "search",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::Internal => "internal",
        }
    }
}

/// Erreur de la façade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingError {
    /// Catégorie.
    pub kind: ErrorKind,
    /// Message lisible.
    pub message: String,
}

impl BindingError {
    /// Erreur de configuration.
    #[must_use]
    pub fn config(message: impl Into<String>) -> Self {
        BindingError {
            kind: ErrorKind::Config,
            message: message.into(),
        }
    }

    /// Erreur interne.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        BindingError {
            kind: ErrorKind::Internal,
            message: message.into(),
        }
    }

    /// Représentation JSON (`{"kind": ..., "message": ...}`).
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::object(vec![
            ("kind", Json::str(self.kind.as_str())),
            ("message", Json::str(&self.message)),
        ])
    }
}

impl fmt::Display for BindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for BindingError {}

impl From<JsonError> for BindingError {
    fn from(e: JsonError) -> Self {
        BindingError::config(e.to_string())
    }
}

impl From<GameError> for BindingError {
    fn from(e: GameError) -> Self {
        BindingError {
            kind: ErrorKind::Game,
            message: e.to_string(),
        }
    }
}

impl From<SearchError> for BindingError {
    fn from(e: SearchError) -> Self {
        let kind = match e {
            SearchError::Cancelled => ErrorKind::Cancelled,
            SearchError::InvalidConfig(_) => ErrorKind::Config,
            _ => ErrorKind::Search,
        };
        BindingError {
            kind,
            message: e.to_string(),
        }
    }
}
