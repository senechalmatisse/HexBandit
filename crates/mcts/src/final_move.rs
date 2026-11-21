//! Choix final du coup après la recherche.

use std::fmt;

/// Résumé d'une action candidate à la racine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CandidateSummary {
    /// N(s, a).
    pub visits: u32,
    /// Valeur de l'action (Q ou Q*).
    pub value: f32,
}

/// Stratégie de choix du coup joué.
pub trait FinalMoveStrategy: fmt::Debug + Send + Sync {
    /// Indice du candidat retenu, ou `None` s'il n'y a aucun candidat.
    fn choose(&self, candidates: &[CandidateSummary]) -> Option<usize>;

    /// Nom court.
    fn name(&self) -> &'static str;
}

/// Coup le plus visité (défaut, robuste) ; égalités départagées par la valeur.
#[derive(Clone, Copy, Debug, Default)]
pub struct MostVisited;

impl FinalMoveStrategy for MostVisited {
    fn choose(&self, candidates: &[CandidateSummary]) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, c) in candidates.iter().enumerate() {
            let better = match best {
                None => true,
                Some(b) => {
                    let cb = candidates[b];
                    c.visits > cb.visits || (c.visits == cb.visits && c.value > cb.value)
                }
            };
            if better {
                best = Some(i);
            }
        }
        best
    }

    fn name(&self) -> &'static str {
        "most_visited"
    }
}

/// Meilleure valeur parmi les actions visitées ; égalités départagées par les
/// visites. Sans action visitée, se replie sur le plus visité.
#[derive(Clone, Copy, Debug, Default)]
pub struct BestValue;

impl FinalMoveStrategy for BestValue {
    fn choose(&self, candidates: &[CandidateSummary]) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, c) in candidates.iter().enumerate() {
            if c.visits == 0 {
                continue;
            }
            let better = match best {
                None => true,
                Some(b) => {
                    let cb = candidates[b];
                    c.value > cb.value || (c.value == cb.value && c.visits > cb.visits)
                }
            };
            if better {
                best = Some(i);
            }
        }
        best.or_else(|| MostVisited.choose(candidates))
    }

    fn name(&self) -> &'static str {
        "best_value"
    }
}

/// « Secure child » : meilleure borne inférieure de confiance
/// Q - a/√N parmi les actions visitées ; sans action visitée, le plus visité.
#[derive(Clone, Copy, Debug)]
pub struct SecureChild {
    /// Coefficient a ≥ 0 (0 : meilleure valeur).
    pub a: f32,
}

impl FinalMoveStrategy for SecureChild {
    fn choose(&self, candidates: &[CandidateSummary]) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (i, c) in candidates.iter().enumerate() {
            if c.visits == 0 {
                continue;
            }
            let bound = c.value - self.a / (c.visits as f32).sqrt();
            let better = match best {
                None => true,
                Some((b, score)) => {
                    bound > score || (bound == score && c.visits > candidates[b].visits)
                }
            };
            if better {
                best = Some((i, bound));
            }
        }
        best.map(|(i, _)| i)
            .or_else(|| MostVisited.choose(candidates))
    }

    fn name(&self) -> &'static str {
        "secure"
    }
}

/// « Max-robust » : le coup à la fois le plus visité et le mieux évalué
/// s'il existe ; sinon celui dont le moins bon des deux rangs (visites,
/// valeur) est le meilleur, à égalité le plus visité. Une recherche sans
/// limite continuerait jusqu'à ce que les deux critères s'accordent ; à
/// budget fixé, ce compromis reste déterministe.
#[derive(Clone, Copy, Debug, Default)]
pub struct MaxRobust;

impl FinalMoveStrategy for MaxRobust {
    fn choose(&self, candidates: &[CandidateSummary]) -> Option<usize> {
        if candidates.is_empty() {
            return None;
        }
        let rank = |key: &dyn Fn(&CandidateSummary) -> f32| -> Vec<usize> {
            let mut order: Vec<usize> = (0..candidates.len()).collect();
            order.sort_by(|&a, &b| {
                key(&candidates[b])
                    .total_cmp(&key(&candidates[a]))
                    .then(a.cmp(&b))
            });
            let mut ranks = vec![0; candidates.len()];
            for (r, &i) in order.iter().enumerate() {
                ranks[i] = r;
            }
            ranks
        };
        let by_visits = rank(&|c| c.visits as f32);
        let by_value = rank(&|c| {
            if c.visits == 0 {
                f32::NEG_INFINITY
            } else {
                c.value
            }
        });
        (0..candidates.len()).min_by_key(|&i| (by_visits[i].max(by_value[i]), by_visits[i]))
    }

    fn name(&self) -> &'static str {
        "max_robust"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategies() {
        let c = [
            CandidateSummary {
                visits: 10,
                value: 0.4,
            },
            CandidateSummary {
                visits: 30,
                value: 0.5,
            },
            CandidateSummary {
                visits: 3,
                value: 0.9,
            },
            CandidateSummary {
                visits: 0,
                value: 1.0,
            },
        ];
        assert_eq!(MostVisited.choose(&c), Some(1));
        assert_eq!(BestValue.choose(&c), Some(2));
        assert_eq!(MostVisited.choose(&[]), None);
        let unvisited = [CandidateSummary {
            visits: 0,
            value: 0.2,
        }];
        assert_eq!(BestValue.choose(&unvisited), Some(0));
        // Secure child : bornes Q - a/√N. Avec a = 1,5 : 0,4 - 0,47 < 0,
        // 0,5 - 0,27 = 0,23 et 0,9 - 0,87 = 0,03 : le deuxième, plus sûr.
        // Avec a = 1 : 0,3174 contre 0,3226, le troisième ; avec a = 0,
        // c'est la meilleure valeur.
        let s = SecureChild { a: 1.5 };
        assert_eq!(s.choose(&c), Some(1));
        assert_eq!(SecureChild { a: 1.0 }.choose(&c), Some(2));
        assert_eq!(SecureChild { a: 0.0 }.choose(&c), Some(2));
        assert_eq!(s.choose(&unvisited), Some(0));
        // Max-robust : le plus visité est aussi le mieux évalué.
        let agree = [
            CandidateSummary {
                visits: 30,
                value: 0.7,
            },
            CandidateSummary {
                visits: 10,
                value: 0.6,
            },
        ];
        assert_eq!(MaxRobust.choose(&agree), Some(0));
        // Désaccord : rang (visites, valeur) = (0, 2), (1, 1), (2, 0) : le
        // deuxième est le meilleur compromis.
        let split = [
            CandidateSummary {
                visits: 30,
                value: 0.40,
            },
            CandidateSummary {
                visits: 25,
                value: 0.55,
            },
            CandidateSummary {
                visits: 5,
                value: 0.90,
            },
        ];
        assert_eq!(MaxRobust.choose(&split), Some(1));
        assert_eq!(MaxRobust.choose(&[]), None);
    }
}
