//! Générateurs pseudo-aléatoires à graine explicite.
//!
//! `rand` et `rand_pcg` n'étant pas disponibles hors ligne, ce module fournit
//! un trait [`Rng`] minimal et le générateur PCG32 (PCG-XSH-RR 64/32 de
//! M. O'Neill), qui gère des flux indépendants par sélection de l'incrément.
//! Il n'y a aucun état global : chaque joueur possède son propre générateur.

/// Source de nombres pseudo-aléatoires.
pub trait Rng {
    /// Prochain entier uniforme sur 32 bits.
    fn next_u32(&mut self) -> u32;

    /// Prochain entier uniforme sur 64 bits.
    #[inline]
    fn next_u64(&mut self) -> u64 {
        let hi = u64::from(self.next_u32());
        let lo = u64::from(self.next_u32());
        (hi << 32) | lo
    }

    /// Entier uniforme dans `[0, bound)`, sans biais (méthode de Lemire).
    /// Renvoie 0 si `bound == 0`.
    #[inline]
    fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        let bound64 = u64::from(bound);
        let mut m = u64::from(self.next_u32()) * bound64;
        let mut low = m as u32;
        if low < bound {
            let threshold = bound.wrapping_neg() % bound;
            while low < threshold {
                m = u64::from(self.next_u32()) * bound64;
                low = m as u32;
            }
        }
        (m >> 32) as u32
    }

    /// Réel uniforme dans `[0, 1)` (53 bits de précision).
    #[inline]
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

impl<R: Rng + ?Sized> Rng for &mut R {
    #[inline]
    fn next_u32(&mut self) -> u32 {
        (**self).next_u32()
    }
}

/// Générateur PCG32 (état 64 bits, sortie 32 bits, flux sélectionnable).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

    /// Crée un générateur à partir d'une graine et d'un identifiant de flux.
    /// Deux flux différents produisent des suites indépendantes, même avec la même graine.
    #[must_use]
    pub fn new(seed: u64, stream: u64) -> Self {
        let mut rng = Pcg32 {
            state: 0,
            inc: (stream << 1) | 1,
        };
        rng.step();
        rng.state = rng.state.wrapping_add(seed);
        rng.step();
        rng
    }

    #[inline]
    fn step(&mut self) {
        self.state = self
            .state
            .wrapping_mul(Self::MULTIPLIER)
            .wrapping_add(self.inc);
    }
}

impl Rng for Pcg32 {
    #[inline]
    fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.step();
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }
}

/// Une étape de SplitMix64 : avance `state` et renvoie une valeur mélangée.
#[inline]
pub fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Dérive de façon déterministe une graine indépendante à partir d'une
/// graine maître et d'un indice.
#[must_use]
pub fn derive_seed(master: u64, index: u64) -> u64 {
    let mut state = master ^ index.wrapping_add(1).wrapping_mul(0xD1B5_4A32_D192_ED03);
    let a = splitmix64(&mut state);
    let b = splitmix64(&mut state);
    a ^ b.rotate_left(29)
}

/// Mélange de Fisher-Yates uniforme.
pub fn shuffle<T, R: Rng + ?Sized>(rng: &mut R, items: &mut [T]) {
    let n = items.len();
    if n < 2 {
        return;
    }
    for i in (1..n).rev() {
        let j = rng.below((i + 1) as u32) as usize;
        items.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcg32_matches_reference_vector() {
        // Valeurs de référence de pcg32-demo (graine 42, séquence 54).
        let mut rng = Pcg32::new(42, 54);
        let expected = [
            0xa15c_02b7_u32,
            0x7b47_f409,
            0xba1d_3330,
            0x83d2_f293,
            0xbfa4_784b,
            0xcbed_606e,
        ];
        for value in expected {
            assert_eq!(rng.next_u32(), value);
        }
    }

    #[test]
    fn streams_are_different() {
        let mut a = Pcg32::new(7, 1);
        let mut b = Pcg32::new(7, 2);
        let same = (0..64).filter(|_| a.next_u32() == b.next_u32()).count();
        assert!(same < 2);
    }

    #[test]
    fn below_is_in_range_and_roughly_uniform() {
        let mut rng = Pcg32::new(1, 1);
        let mut counts = [0u32; 7];
        for _ in 0..70_000 {
            let v = rng.below(7);
            counts[v as usize] += 1;
        }
        for c in counts {
            assert!((9_000..11_000).contains(&c), "{counts:?}");
        }
        assert_eq!(rng.below(0), 0);
        assert_eq!(rng.below(1), 0);
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut rng = Pcg32::new(3, 3);
        let mut v: Vec<u32> = (0..100).collect();
        shuffle(&mut rng, &mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..100).collect::<Vec<_>>());
        assert_ne!(v, sorted);
    }

    #[test]
    fn derived_seeds_are_distinct_and_stable() {
        let seeds: Vec<u64> = (0..1000).map(|i| derive_seed(12345, i)).collect();
        let mut dedup = seeds.clone();
        dedup.sort_unstable();
        dedup.dedup();
        assert_eq!(dedup.len(), seeds.len());
        assert_eq!(derive_seed(12345, 3), seeds[3]);
        let f = Pcg32::new(1, 1).next_f64();
        assert!((0.0..1.0).contains(&f));
    }
}
