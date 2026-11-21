//! Abstractions communes aux jeux combinatoires à deux joueurs.
//!
//! Ce crate ne dépend d'aucun jeu ni d'aucun algorithme de recherche. Il définit :
//!
//! * les joueurs et résultats ([`Player`], [`Outcome`]) et les erreurs ([`GameError`]) ;
//! * les interfaces de règles ([`Game`], [`AmafGame`]) ;
//! * l'aléatoire à graine explicite ([`rng`]) ;
//! * des fonctions mathématiques identiques sur toutes les plateformes ([`math`]) ;
//! * les politiques de simulation ([`rollout`]) ;
//! * des décorateurs génériques ([`decorators`] : misère, règle du gâteau) ;
//! * des tests de contrat réutilisables ([`contract`]).
#![forbid(unsafe_code)]

pub mod contract;
pub mod decorators;
mod error;
mod game;
pub mod math;
mod player;
pub mod rng;
pub mod rollout;

pub use error::GameError;
pub use game::{AmafGame, Game};
pub use player::{Outcome, Player};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decorators::{Misere, MisereRollout, PieGame, SwapMove, SwapRollout, SwapRule};
    use crate::rollout::UniformRollout;

    /// Jeu jouet pour tester le noyau : Nim à un tas (prendre 1 à 3 objets),
    /// celui qui prend le dernier gagne.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Nim {
        heap: u8,
        to_move: Player,
        ply: u32,
        last: Option<Player>,
    }

    impl Nim {
        fn new(heap: u8) -> Self {
            Nim {
                heap,
                to_move: Player::P1,
                ply: 0,
                last: None,
            }
        }
    }

    impl Game for Nim {
        type Move = u8;
        fn current_player(&self) -> Player {
            self.to_move
        }
        fn legal_moves(&self, out: &mut Vec<u8>) {
            out.clear();
            if self.heap > 0 {
                out.extend((1..=3u8).filter(|&k| k <= self.heap));
            }
        }
        fn play(&mut self, mv: u8) -> Result<(), GameError> {
            if self.heap == 0 {
                return Err(GameError::GameOver);
            }
            if mv == 0 || mv > 3 || mv > self.heap {
                return Err(GameError::IllegalMove {
                    mv: mv.to_string(),
                    reason: "taille".into(),
                });
            }
            self.heap -= mv;
            self.last = Some(self.to_move);
            self.to_move = self.to_move.opponent();
            self.ply += 1;
            Ok(())
        }
        fn outcome(&self) -> Option<Outcome> {
            if self.heap == 0 {
                self.last.map(Outcome::Win)
            } else {
                None
            }
        }
        fn ply(&self) -> u32 {
            self.ply
        }
    }

    impl AmafGame for Nim {
        fn move_index(&self, mv: u8) -> usize {
            usize::from(mv - 1)
        }
        fn move_index_bound(&self) -> usize {
            3
        }
    }

    impl PieGame for Nim {
        fn apply_swap(&mut self) -> Result<(), GameError> {
            // Échange trivial : le tour revient au premier joueur.
            self.to_move = self.to_move.opponent();
            self.ply += 1;
            Ok(())
        }
    }

    #[test]
    fn nim_satisfies_contracts() {
        let nim = Nim::new(15);
        contract::check_game_contract(&nim, 1, 50, 100).unwrap();
        contract::check_amaf_contract(&nim, 2, 50).unwrap();
        contract::check_rollout_contract(&nim, &mut UniformRollout::new(), 3, 50).unwrap();
    }

    #[test]
    fn misere_inverts_outcome_and_satisfies_contracts() {
        let mut m = Misere::new(Nim::new(3));
        m.play(3).unwrap();
        assert_eq!(m.outcome(), Some(Outcome::Win(Player::P2)));
        let m = Misere::new(Nim::new(15));
        contract::check_game_contract(&m, 4, 50, 100).unwrap();
        let mut policy = MisereRollout::new(UniformRollout::<Nim>::new());
        contract::check_rollout_contract(&m, &mut policy, 5, 50).unwrap();
    }

    #[test]
    fn swap_rule_offers_swap_only_at_second_move() {
        let mut s = SwapRule::new(Nim::new(10));
        let mut moves = Vec::new();
        s.legal_moves(&mut moves);
        assert!(!moves.contains(&SwapMove::Swap));
        s.play(SwapMove::Place(1)).unwrap();
        s.legal_moves(&mut moves);
        assert!(moves.contains(&SwapMove::Swap));
        s.play(SwapMove::Swap).unwrap();
        assert!(s.swapped());
        assert!(s.play(SwapMove::Swap).is_err());
        let s = SwapRule::new(Nim::new(15));
        contract::check_game_contract(&s, 6, 50, 100).unwrap();
        contract::check_amaf_contract(&s, 7, 50).unwrap();
        let mut policy = SwapRollout::new(UniformRollout::<Nim>::new());
        contract::check_rollout_contract(&s, &mut policy, 8, 50).unwrap();
    }
}
