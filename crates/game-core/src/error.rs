//! Erreurs typées des règles de jeu.
//!
//! Remplace `thiserror` (indisponible hors ligne) par une implémentation
//! manuelle de [`std::fmt::Display`] et [`std::error::Error`].

use std::fmt;

/// Erreur renvoyée par les règles d'un jeu. Aucune règle ne panique sur une
/// entrée invalide : elle renvoie l'une de ces variantes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GameError {
    /// La partie est déjà terminée.
    GameOver,
    /// Le coup désigne une case hors du plateau.
    OutOfBounds {
        /// Représentation textuelle du coup.
        mv: String,
    },
    /// La case visée est déjà occupée.
    Occupied {
        /// Représentation textuelle du coup.
        mv: String,
    },
    /// Le coup est illégal pour une autre raison.
    IllegalMove {
        /// Représentation textuelle du coup.
        mv: String,
        /// Explication.
        reason: String,
    },
    /// Une notation (coup ou position) n'a pas pu être lue.
    Parse {
        /// Texte fautif.
        input: String,
        /// Explication.
        reason: String,
    },
    /// Un paramètre de construction est invalide (taille de plateau, etc.).
    InvalidParameter {
        /// Nom du paramètre.
        name: &'static str,
        /// Explication.
        reason: String,
    },
}

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GameError::GameOver => write!(f, "la partie est terminée"),
            GameError::OutOfBounds { mv } => write!(f, "coup {mv} hors du plateau"),
            GameError::Occupied { mv } => write!(f, "case {mv} déjà occupée"),
            GameError::IllegalMove { mv, reason } => write!(f, "coup {mv} illégal : {reason}"),
            GameError::Parse { input, reason } => {
                write!(f, "lecture impossible de {input:?} : {reason}")
            }
            GameError::InvalidParameter { name, reason } => {
                write!(f, "paramètre {name} invalide : {reason}")
            }
        }
    }
}

impl std::error::Error for GameError {}
