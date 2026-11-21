//! Géométrie du plateau en losange : voisinage précalculé pour chaque taille.
//!
//! La case `(r, c)` (ligne `r`, colonne `c`) a six voisins au plus, aux
//! décalages `(±1, 0)`, `(0, ±1)`, `(+1, -1)` et `(-1, +1)`. Les
//! tables sont construites une seule fois par taille et partagées en lecture.

use std::sync::OnceLock;

use crate::{MAX_SIZE, MIN_SIZE};

/// Marqueur d'absence de voisin.
pub(crate) const NO_CELL: u16 = u16::MAX;

/// Décalages `(dr, dc)` des six voisins.
pub(crate) const NEIGHBOR_OFFSETS: [(isize, isize); 6] =
    [(-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0)];

/// Bit « bord Ouest » (colonne 0).
pub(crate) const WEST: u8 = 1;
/// Bit « bord Est » (colonne n - 1).
pub(crate) const EAST: u8 = 2;
/// Bit « bord Nord » (ligne 0).
pub(crate) const NORTH: u8 = 4;
/// Bit « bord Sud » (ligne n - 1).
pub(crate) const SOUTH: u8 = 8;

/// Table de voisinage d'un plateau `n × n`.
#[derive(Debug)]
pub(crate) struct Topology {
    pub(crate) n: usize,
    pub(crate) neighbors: Vec<[u16; 6]>,
    /// Voisins complétés jusqu'à 6 par la case sentinelle `n²` (hors plateau,
    /// toujours vide dans les tampons des simulations) : les boucles chaudes
    /// font toujours 6 tours, sans branche dépendant du degré.
    pub(crate) padded: Vec<[u16; 6]>,
    pub(crate) degree: Vec<u8>,
    /// Bords touchés par chaque case (bits WEST, EAST, NORTH, SOUTH) : évite
    /// toute division entière dans les boucles chaudes.
    pub(crate) borders: Vec<u8>,
}

impl Topology {
    fn build(n: usize) -> Topology {
        let mut neighbors = vec![[NO_CELL; 6]; n * n];
        let mut degree = vec![0u8; n * n];
        let mut borders = vec![0u8; n * n];
        for r in 0..n {
            for c in 0..n {
                let cell = r * n + c;
                let mut k = 0;
                for (dr, dc) in NEIGHBOR_OFFSETS {
                    let (nr, nc) = (r as isize + dr, c as isize + dc);
                    if nr >= 0 && nc >= 0 && (nr as usize) < n && (nc as usize) < n {
                        neighbors[cell][k] = (nr as usize * n + nc as usize) as u16;
                        k += 1;
                    }
                }
                degree[cell] = k as u8;
                let mut b = 0;
                if c == 0 {
                    b |= WEST;
                }
                if c == n - 1 {
                    b |= EAST;
                }
                if r == 0 {
                    b |= NORTH;
                }
                if r == n - 1 {
                    b |= SOUTH;
                }
                borders[cell] = b;
            }
        }
        let sentinel = (n * n) as u16;
        let padded = neighbors
            .iter()
            .zip(&degree)
            .map(|(row, &d)| {
                let mut out = [sentinel; 6];
                out[..usize::from(d)].copy_from_slice(&row[..usize::from(d)]);
                out
            })
            .collect();
        Topology {
            n,
            neighbors,
            padded,
            degree,
            borders,
        }
    }

    /// Bits (premier bord, second bord) du joueur : Ouest/Est pour Bleu, Nord/Sud pour Rouge.
    #[inline]
    pub(crate) fn player_borders(player: game_core::Player) -> (u8, u8) {
        // Table plutôt que `match` : pas de branchement dans la simulation.
        const BORDERS: [(u8, u8); 2] = [(WEST, EAST), (NORTH, SOUTH)];
        BORDERS[player.index()]
    }

    /// Voisins de la case `cell`.
    #[inline]
    pub(crate) fn neighbors(&self, cell: usize) -> &[u16] {
        &self.neighbors[cell][..self.degree[cell] as usize]
    }

    /// Voisins de `cell` complétés par la sentinelle `n²` (voir `padded`).
    #[inline]
    pub(crate) fn neighbors_padded(&self, cell: usize) -> &[u16; 6] {
        &self.padded[cell]
    }
}

/// Géométrie publique d'un plateau `n × n` en losange (voisinages et bords
/// précalculés, partagés en lecture), pour les variantes qui réutilisent le
/// plateau du Hex sans ses règles (Chameleon).
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    topo: &'static Topology,
}

impl Geometry {
    /// Bord Ouest (colonne 0).
    pub const WEST: u8 = WEST;
    /// Bord Est (colonne n - 1).
    pub const EAST: u8 = EAST;
    /// Bord Nord (ligne 0).
    pub const NORTH: u8 = NORTH;
    /// Bord Sud (ligne n - 1).
    pub const SOUTH: u8 = SOUTH;

    /// Géométrie du plateau de taille `size`.
    ///
    /// # Errors
    ///
    /// [`game_core::GameError::InvalidParameter`] si la taille n'est pas dans
    /// `[MIN_SIZE, MAX_SIZE]`.
    pub fn new(size: usize) -> Result<Geometry, game_core::GameError> {
        if !(MIN_SIZE..=MAX_SIZE).contains(&size) {
            return Err(game_core::GameError::InvalidParameter {
                name: "size",
                reason: format!("{size} hors de [{MIN_SIZE}, {MAX_SIZE}]"),
            });
        }
        Ok(Geometry {
            topo: topology(size),
        })
    }

    /// Côté du plateau.
    #[inline]
    #[must_use]
    pub fn size(&self) -> usize {
        self.topo.n
    }

    /// Voisins de la case d'indice `cell` (`ligne × n + colonne`).
    #[inline]
    #[must_use]
    pub fn neighbors(&self, cell: usize) -> &'static [u16] {
        self.topo.neighbors(cell)
    }

    /// Bords touchés par la case (bits [`Geometry::WEST`], [`Geometry::EAST`],
    /// [`Geometry::NORTH`], [`Geometry::SOUTH`]).
    #[inline]
    #[must_use]
    pub fn borders(&self, cell: usize) -> u8 {
        self.topo.borders[cell]
    }
}

static TOPOLOGIES: OnceLock<Vec<Topology>> = OnceLock::new();

/// Table de voisinage partagée pour la taille `n` (supposée valide).
pub(crate) fn topology(n: usize) -> &'static Topology {
    let all = TOPOLOGIES.get_or_init(|| (MIN_SIZE..=MAX_SIZE).map(Topology::build).collect());
    &all[n - MIN_SIZE]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degrees_and_symmetry() {
        for n in MIN_SIZE..=MAX_SIZE {
            let t = topology(n);
            assert_eq!(t.n, n);
            // Coins aigus (haut-gauche, bas-droite) : 2 voisins ; coins obtus : 3.
            assert_eq!(t.degree[0], 2);
            assert_eq!(t.degree[n * n - 1], 2);
            assert_eq!(t.degree[n - 1], 3);
            assert_eq!(t.degree[(n - 1) * n], 3);
            for cell in 0..n * n {
                for &nb in t.neighbors(cell) {
                    assert!(t.neighbors(nb as usize).contains(&(cell as u16)));
                }
            }
        }
        let t = topology(14);
        let center = 7 * 14 + 7;
        assert_eq!(t.degree[center], 6);
        for n in MIN_SIZE..=MAX_SIZE {
            let t = topology(n);
            for cell in 0..n * n {
                let d = usize::from(t.degree[cell]);
                assert_eq!(&t.neighbors_padded(cell)[..d], t.neighbors(cell));
                assert!(
                    t.neighbors_padded(cell)[d..]
                        .iter()
                        .all(|&s| usize::from(s) == n * n)
                );
            }
        }
    }
}
