//! Budgets de recherche.

use std::{
    fmt,
    time::{Duration, Instant},
};

/// Budget d'une recherche : autorise-t-on une simulation de plus ?
pub trait Budget: fmt::Debug + Send {
    /// Appelé au début de chaque recherche.
    fn start(&mut self) {}

    /// Vrai si une simulation supplémentaire est autorisée après `sims_done`.
    fn allows_next(&mut self, sims_done: u64) -> bool;

    /// Faux si le budget dépend du temps (résultats non reproductibles).
    fn is_deterministic(&self) -> bool {
        true
    }

    /// Nombre de simulations attendu, s'il est connu d'avance : le moteur
    /// réserve alors la mémoire de l'arbre en une fois au lieu de la faire
    /// croître par réallocations successives. Sans effet sur les résultats.
    fn expected_simulations(&self) -> Option<u64> {
        None
    }

    /// Description lisible.
    fn describe(&self) -> String;

    /// Part `index` (sur `parts`) du budget, pour une recherche parallèle à
    /// la racine : les simulations sont réparties (les premières parts en
    /// reçoivent une de plus si la division ne tombe pas juste), le temps est
    /// le même pour chaque part. `None` si le budget ne se partage pas.
    fn share(&self, _index: usize, _parts: usize) -> Option<Box<dyn Budget>> {
        None
    }
}

/// Nombre fixe de simulations par coup (défaut, déterministe).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimulationBudget {
    /// Simulations par coup.
    pub simulations: u64,
}

impl Budget for SimulationBudget {
    #[inline]
    fn allows_next(&mut self, sims_done: u64) -> bool {
        sims_done < self.simulations
    }

    fn expected_simulations(&self) -> Option<u64> {
        Some(self.simulations)
    }

    fn describe(&self) -> String {
        format!("{} simulations/coup", self.simulations)
    }

    fn share(&self, index: usize, parts: usize) -> Option<Box<dyn Budget>> {
        let parts = parts.max(1) as u64;
        let index = index as u64;
        let base = self.simulations / parts;
        let extra = u64::from(index < self.simulations % parts);
        Some(Box::new(SimulationBudget {
            simulations: base + extra,
        }))
    }
}

/// Temps fixe par coup (variante [C], non déterministe).
#[derive(Clone, Copy, Debug)]
pub struct TimeBudget {
    /// Durée par coup.
    pub limit: Duration,
    /// Nombre minimal de simulations, même si le temps est écoulé.
    pub min_simulations: u64,
    started: Option<Instant>,
}

impl TimeBudget {
    /// Budget en temps.
    #[must_use]
    pub fn new(limit: Duration) -> Self {
        TimeBudget {
            limit,
            min_simulations: 1,
            started: None,
        }
    }
}

impl Budget for TimeBudget {
    fn start(&mut self) {
        self.started = Some(Instant::now());
    }

    #[inline]
    fn allows_next(&mut self, sims_done: u64) -> bool {
        if sims_done < self.min_simulations {
            return true;
        }
        // Lecture de l'horloge toutes les 64 simulations seulement.
        if sims_done % 64 != 0 {
            return true;
        }
        match self.started {
            Some(t) => t.elapsed() < self.limit,
            None => false,
        }
    }

    fn is_deterministic(&self) -> bool {
        false
    }

    fn describe(&self) -> String {
        format!("{} ms/coup", self.limit.as_millis())
    }

    fn share(&self, _index: usize, _parts: usize) -> Option<Box<dyn Budget>> {
        Some(Box::new(TimeBudget {
            limit: self.limit,
            min_simulations: self.min_simulations,
            started: None,
        }))
    }
}
