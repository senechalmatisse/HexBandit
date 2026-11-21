//! Plannings de β pour MC-RAVE, interchangeables à l'exécution.
//!
//! La valeur combinée est Q*(s,a) = (1 - β)·Q(s,a) + β·Q̃(s,a) (éq. 12).

use std::fmt;

/// Planning du poids β donné à la valeur AMAF.
///
/// Les arguments sont N(s) (visites du nœud parent), N(s,a) et Ñ(s,a).
/// L'appelant ne consulte le planning que si N(s,a) > 0 et Ñ(s,a) > 0 ; si
/// N(s,a) = 0, la valeur Monte-Carlo n'existe pas et β vaut 1.
pub trait BetaSchedule: fmt::Debug + Send + Sync {
    /// Poids β dans `[0, 1]`.
    fn beta(&self, parent_visits: u32, n: u32, n_amaf: u32) -> f32;

    /// Forme du planning pour un nœud de N(s) = `parent_visits` visites,
    /// consultée une fois par sélection pour sortir la répartition de la
    /// boucle sur les arêtes. Toute forme doit donner exactement la même
    /// valeur que [`BetaSchedule::beta`] ; par défaut, [`ScheduleKind::General`].
    #[inline]
    fn kind(&self, _parent_visits: u32) -> ScheduleKind {
        ScheduleKind::General
    }
}

/// Forme d'un planning de β pour un nœud donné.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScheduleKind {
    /// β ne dépend pas de l'arête (éq. 19, ou β = 0).
    Constant(f32),
    /// β = Ñ / (N + Ñ + 4·N·Ñ·b²) (éq. 47).
    MinimumMse {
        /// Biais RAVE b.
        bias: f32,
    },
    /// Appeler [`BetaSchedule::beta`] pour chaque arête.
    General,
}

/// Planning manuel (éq. 19) : β = √(k / (3·N(s) + k)). Le paramètre `k` est
/// le nombre de simulations pour lequel les deux estimations ont le même poids.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandSelected {
    /// Paramètre d'équivalence k (> 0).
    pub k: f32,
}

impl BetaSchedule for HandSelected {
    #[inline]
    fn beta(&self, parent_visits: u32, _n: u32, _n_amaf: u32) -> f32 {
        (self.k / (3.0 * parent_visits as f32 + self.k)).sqrt()
    }

    #[inline]
    fn kind(&self, parent_visits: u32) -> ScheduleKind {
        ScheduleKind::Constant(self.beta(parent_visits, 0, 0))
    }
}

/// Planning à erreur quadratique minimale (éq. 47) :
/// β = Ñ / (N + Ñ + 4·N·Ñ·b²), avec b le biais RAVE réglé empiriquement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinimumMse {
    /// Biais RAVE b (≥ 0).
    pub bias: f32,
}

impl BetaSchedule for MinimumMse {
    #[inline]
    fn beta(&self, _parent_visits: u32, n: u32, n_amaf: u32) -> f32 {
        let (n, na) = (n as f32, n_amaf as f32);
        na / (n + na + 4.0 * n * na * self.bias * self.bias)
    }

    #[inline]
    fn kind(&self, _parent_visits: u32) -> ScheduleKind {
        ScheduleKind::MinimumMse { bias: self.bias }
    }
}

/// Objet nul : β = 0, ce qui ramène MC-RAVE à UCT pur (statistiques AMAF
/// maintenues mais ignorées).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NoRave;

impl BetaSchedule for NoRave {
    #[inline]
    fn beta(&self, _parent_visits: u32, _n: u32, _n_amaf: u32) -> f32 {
        0.0
    }

    #[inline]
    fn kind(&self, _parent_visits: u32) -> ScheduleKind {
        ScheduleKind::Constant(0.0)
    }
}

/// Planning choisi à l'exécution (configuration) parmi les trois ci-dessus.
/// La sélection consulte [`BetaSchedule::kind`] une fois par nœud : la boucle
/// sur les arêtes ne contient ni appel dynamique ni `match` sur le planning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnySchedule {
    /// Éq. 19.
    HandSelected(HandSelected),
    /// Éq. 47.
    MinimumMse(MinimumMse),
    /// β = 0.
    None(NoRave),
}

impl BetaSchedule for AnySchedule {
    #[inline]
    fn beta(&self, parent_visits: u32, n: u32, n_amaf: u32) -> f32 {
        match self {
            AnySchedule::HandSelected(s) => s.beta(parent_visits, n, n_amaf),
            AnySchedule::MinimumMse(s) => s.beta(parent_visits, n, n_amaf),
            AnySchedule::None(s) => s.beta(parent_visits, n, n_amaf),
        }
    }

    #[inline]
    fn kind(&self, parent_visits: u32) -> ScheduleKind {
        match self {
            AnySchedule::HandSelected(s) => s.kind(parent_visits),
            AnySchedule::MinimumMse(s) => s.kind(parent_visits),
            AnySchedule::None(s) => s.kind(parent_visits),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_selected_is_one_half_at_k() {
        let s = HandSelected { k: 1000.0 };
        assert!((s.beta(1000, 1, 1) - 0.5).abs() < 1e-6);
        assert!((s.beta(0, 0, 0) - 1.0).abs() < 1e-6);
        assert!(s.beta(1_000_000, 1, 1) < 0.02);
    }

    #[test]
    fn minimum_mse_limits() {
        let s = MinimumMse { bias: 0.0 };
        assert!((s.beta(0, 10, 30) - 0.75).abs() < 1e-6);
        let s = MinimumMse { bias: 0.1 };
        // Beaucoup de simulations MC : β tend vers 0.
        assert!(s.beta(0, 100_000, 100_000) < 0.01);
        // Aucune simulation MC : β = 1.
        assert!((s.beta(0, 0, 50) - 1.0).abs() < 1e-6);
        assert_eq!(NoRave.beta(5, 5, 5), 0.0);
        let any = AnySchedule::MinimumMse(s);
        assert_eq!(any.beta(1, 2, 3), s.beta(1, 2, 3));
    }
}
