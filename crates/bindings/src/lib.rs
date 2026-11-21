//! Façade native de hexbandit : frontière entre le moteur Rust et Python.
//!
//! * [`config`] : configurations versionnées, validées (source de vérité) ;
//! * [`registry`] : assemblage des jeux et des joueurs (seul point de
//!   composition, Abstract Factory + registre) ;
//! * [`facade`] : `play_game`, `play_batch`, `search` en JSON ;
//! * [`ffi`] : ABI C chargée par `ctypes`.
//!
//! C'est le seul crate qui contient du code `unsafe` (lecture des pointeurs
//! C dans [`ffi`]) et des entrées-sorties (journal sur la sortie d'erreur).
#![deny(unsafe_op_in_unsafe_fn)]

pub mod config;
pub mod error;
pub mod facade;
pub mod ffi;
pub mod json;
mod log;
mod registry;
