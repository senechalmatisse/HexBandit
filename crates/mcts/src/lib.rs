//! Recherche arborescente de Monte-Carlo générique : UCT et MC-RAVE.
//!
//! Le moteur [`Mcts`] est générique sur le jeu (`G: Game`), l'algorithme
//! ([`Uct`] ou [`Rave`]) et la politique de simulation ; tout est
//! monomorphisé sur le chemin chaud. Les stratégies interchangeables
//! (planning de β, budget, choix final) sont des traits ; l'assemblage se fait par [`MctsBuilder`].
//!
//! Correspondance avec l'article de Gelly et Silver :
//!
//! | Article | Code |
//! |---|---|
//! | éq. 4-6 (mise à jour MC) | `Mcts::simulate`, étape 4 |
//! | éq. 7-8 (UCB1) | [`Uct`] |
//! | éq. 12 (Q*) | [`Rave`] |
//! | éq. 16-17, algorithme 2 (AMAF) | `Rave::backup_extra` |
//! | éq. 18 (UCT-RAVE) | [`Rave`] avec `exploration > 0` |
//! | éq. 19 (planning manuel) | [`beta::HandSelected`] |
//! | éq. 47 (erreur quadratique minimale) | [`beta::MinimumMse`] |
//! | éq. 48-52 (connaissance a priori) | [`prior`] |
//!
//! Ce crate ne fait aucune entrée-sortie.
#![forbid(unsafe_code)]

pub mod agent;
mod algorithm;
pub mod beta;
pub mod budget;
mod builder;
mod cancel;
pub mod final_move;
pub mod observer;
pub mod prior;
mod report;
pub mod runner;
mod search;
pub mod tree;

pub use agent::{Agent, MctsAgent, RandomAgent, RootParallelAgent};
pub use algorithm::{Exclusion, PathStep, Rave, TreeAlgorithm, Uct};
pub use builder::MctsBuilder;
pub use cancel::CancelToken;
pub use report::{CandidateStats, ReportDetail, SearchError, SearchReport};
pub use search::{Mcts, SearchConfig};
