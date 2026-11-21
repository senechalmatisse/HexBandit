//! Jeton d'annulation partagé.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Jeton d'annulation clonable et partageable entre threads.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// Nouveau jeton, non annulé.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Demande l'annulation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Réarme le jeton.
    pub fn reset(&self) {
        self.0.store(false, Ordering::Relaxed);
    }

    /// Vrai si l'annulation a été demandée.
    #[inline]
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
