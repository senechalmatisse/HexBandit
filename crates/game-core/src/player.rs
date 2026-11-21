//! Joueurs et résultats de partie.

/// Identité d'un joueur dans un jeu à deux joueurs.
///
/// L'identité est indépendante de l'ordre de jeu : au Hex, [`Player::P1`] est
/// Bleu et [`Player::P2`] est Rouge, quel que soit celui qui commence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Player {
    /// Premier joueur par identité (Bleu au Hex).
    P1,
    /// Second joueur par identité (Rouge au Hex).
    P2,
}

impl Player {
    /// Les deux joueurs, dans l'ordre des indices.
    pub const BOTH: [Player; 2] = [Player::P1, Player::P2];

    /// Adversaire du joueur.
    #[inline]
    #[must_use]
    pub const fn opponent(self) -> Player {
        match self {
            Player::P1 => Player::P2,
            Player::P2 => Player::P1,
        }
    }

    /// Indice du joueur (0 pour `P1`, 1 pour `P2`), utile pour indexer des tableaux.
    #[inline]
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Player::P1 => 0,
            Player::P2 => 1,
        }
    }

    /// Joueur correspondant à un indice (0 ou 1).
    #[must_use]
    pub const fn from_index(index: usize) -> Option<Player> {
        match index {
            0 => Some(Player::P1),
            1 => Some(Player::P2),
            _ => None,
        }
    }
}

/// Résultat d'une partie terminée.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// Victoire du joueur indiqué.
    Win(Player),
    /// Match nul (impossible au Hex, possible au Quarto).
    Draw,
}

impl Outcome {
    /// Récompense dans `[0, 1]` du point de vue de `player` :
    /// 1 pour une victoire, 0,5 pour un nul, 0 pour une défaite.
    #[inline]
    #[must_use]
    pub fn reward_for(self, player: Player) -> f32 {
        match self {
            Outcome::Win(winner) if winner == player => 1.0,
            Outcome::Win(_) => 0.0,
            Outcome::Draw => 0.5,
        }
    }

    /// Vainqueur éventuel.
    #[must_use]
    pub const fn winner(self) -> Option<Player> {
        match self {
            Outcome::Win(p) => Some(p),
            Outcome::Draw => None,
        }
    }

    /// Résultat inversé (variante « misère » : le vainqueur devient le perdant).
    #[must_use]
    pub const fn inverted(self) -> Outcome {
        match self {
            Outcome::Win(p) => Outcome::Win(p.opponent()),
            Outcome::Draw => Outcome::Draw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opponent_is_an_involution() {
        for p in Player::BOTH {
            assert_eq!(p.opponent().opponent(), p);
            assert_ne!(p.opponent(), p);
            assert_eq!(Player::from_index(p.index()), Some(p));
        }
        assert_eq!(Player::from_index(2), None);
    }

    #[test]
    fn rewards_are_zero_sum() {
        for o in [
            Outcome::Win(Player::P1),
            Outcome::Win(Player::P2),
            Outcome::Draw,
        ] {
            let total = o.reward_for(Player::P1) + o.reward_for(Player::P2);
            assert!((total - 1.0).abs() < f32::EPSILON);
        }
        assert_eq!(
            Outcome::Win(Player::P1).inverted(),
            Outcome::Win(Player::P2)
        );
        assert_eq!(Outcome::Draw.inverted(), Outcome::Draw);
    }
}
