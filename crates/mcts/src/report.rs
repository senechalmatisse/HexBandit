//! Rapport de recherche par coup et erreurs du moteur.

use std::fmt;

use game_core::GameError;

/// Niveau de détail du rapport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReportDetail {
    /// Compteurs globaux seulement (campagnes).
    #[default]
    Summary,
    /// Compteurs globaux et statistiques de chaque coup candidat (visualisation).
    Full,
}

/// Statistiques d'un coup candidat à la racine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CandidateStats<M> {
    /// Coup.
    pub mv: M,
    /// N(s, a).
    pub visits: u32,
    /// Q(s, a).
    pub q: f32,
    /// Ñ(s, a) (0 pour UCT).
    pub amaf_visits: u32,
    /// Q̃(s, a) (0 pour UCT).
    pub amaf_q: f32,
    /// Valeur utilisée par l'algorithme (Q pour UCT, Q* pour RAVE).
    pub value: f32,
    /// Valeur prouvée du coup par le solveur, en demi-points (0 défaite,
    /// 1 nul, 2 victoire), si elle est connue.
    pub proven: Option<u8>,
}

/// Ordre des candidats dans un rapport : coups prouvés gagnants d'abord et
/// prouvés perdants en dernier (solveur), puis visites et valeur décroissantes.
pub(crate) fn candidate_order<M>(
    a: &CandidateStats<M>,
    b: &CandidateStats<M>,
) -> std::cmp::Ordering {
    let rank = |c: &CandidateStats<M>| match c.proven {
        Some(2) => 3,
        None => 2,
        Some(1) => 1,
        Some(_) => 0,
    };
    rank(b)
        .cmp(&rank(a))
        .then(b.visits.cmp(&a.visits))
        .then(b.value.total_cmp(&a.value))
}

/// Rapport d'une recherche.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchReport<M> {
    /// Coup choisi.
    pub best_move: M,
    /// Simulations effectuées.
    pub simulations: u64,
    /// Nœuds de l'arbre en fin de recherche.
    pub nodes: u32,
    /// Profondeur maximale atteinte dans l'arbre (nombre de nœuds traversés).
    pub max_depth: u32,
    /// Profondeur moyenne des simulations dans l'arbre.
    pub mean_depth: f32,
    /// Récompense moyenne des simulations, du point de vue du joueur au trait.
    pub root_value: f32,
    /// Visites du coup choisi.
    pub best_visits: u32,
    /// Valeur du coup choisi.
    pub best_value: f32,
    /// Vrai si la limite de nœuds a été atteinte.
    pub node_limit_reached: bool,
    /// Durée de la recherche en microsecondes (non déterministe).
    pub elapsed_us: u64,
    /// Visites de la racine héritées de la recherche précédente (réutilisation
    /// de l'arbre, 0 sinon) : le budget effectif est `simulations + reused_visits`.
    pub reused_visits: u32,
    /// Valeur prouvée de la position pour le joueur au trait (solveur), en
    /// demi-points (0 défaite, 1 nul, 2 victoire).
    pub proven: Option<u8>,
    /// Candidats (vide sauf en [`ReportDetail::Full`]), triés par visites décroissantes.
    pub candidates: Vec<CandidateStats<M>>,
}

impl<M> SearchReport<M> {
    /// Rapport minimal d'un coup choisi sans recherche (joueur aléatoire).
    #[must_use]
    pub fn without_search(best_move: M) -> Self {
        SearchReport {
            best_move,
            simulations: 0,
            nodes: 0,
            max_depth: 0,
            mean_depth: 0.0,
            root_value: 0.5,
            best_visits: 0,
            best_value: 0.5,
            node_limit_reached: false,
            elapsed_us: 0,
            reused_visits: 0,
            proven: None,
            candidates: Vec::new(),
        }
    }
}

/// Erreur du moteur de recherche ou de l'orchestrateur.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SearchError {
    /// Recherche demandée sur une position terminale.
    TerminalPosition,
    /// Recherche ou partie interrompue (Ctrl-C).
    Cancelled,
    /// Un coup illégal a été produit (bug d'un joueur ou du moteur).
    IllegalMove(GameError),
    /// La partie dépasse le nombre maximal de coups autorisé.
    PlyLimit(u32),
    /// Configuration invalide.
    InvalidConfig(String),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchError::TerminalPosition => write!(f, "la position est terminale"),
            SearchError::Cancelled => write!(f, "recherche interrompue"),
            SearchError::IllegalMove(e) => write!(f, "coup illégal produit par le moteur : {e}"),
            SearchError::PlyLimit(n) => write!(f, "partie trop longue (plus de {n} coups)"),
            SearchError::InvalidConfig(m) => write!(f, "configuration invalide : {m}"),
        }
    }
}

impl std::error::Error for SearchError {}

impl From<GameError> for SearchError {
    fn from(e: GameError) -> Self {
        SearchError::IllegalMove(e)
    }
}
