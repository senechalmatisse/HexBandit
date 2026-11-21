//! Fonctions mathématiques déterministes, identiques au bit près sur toutes les plateformes.
//!
//! Les opérations de base de l'IEEE 754 (addition, multiplication, division,
//! racine carrée, conversions) sont correctement arrondies : leur résultat ne
//! dépend ni du processeur ni du système. Les fonctions transcendantes comme
//! `f32::ln`, en revanche, appellent la bibliothèque mathématique du système
//! (glibc, libm de macOS, CRT de Windows, musl…), dont les arrondis diffèrent.
//! UCT, qui utilise ln N(s) dans sa formule de sélection, pouvait donc jouer
//! des parties différentes selon la plateforme.
//!
//! [`ln`] est un portage de `logf` de la glibc (bibliothèque *optimized
//! routines* d'Arm, utilisée par la glibc depuis la version 2.28) écrit
//! uniquement avec des opérations de base en double précision : il donne le
//! même résultat partout. Il a été comparé à `logf` de la glibc 2.39 sur
//! **toutes** les valeurs `f32` positives finies (2 139 095 039 valeurs, y
//! compris les dénormaux) : aucun écart, ni avec la variante FMA (choisie par
//! la glibc sur les processeurs récents) ni avec la variante sans FMA.
//!
//! Erreur maximale annoncée par les auteurs : 0,82 ULP (arrondi au plus près).

/// Table des 16 sous-intervalles : (1/c, ln c) en bits IEEE 754 double
/// précision, valeurs de la glibc (`__logf_data.tab`).
const TABLE: [(u64, u64); 16] = [
    (0x3ff6_61ec_79f8_f3be, 0xbfd5_7bf7_808c_aade),
    (0x3ff5_71ed_4aaf_883d, 0xbfd2_bef0_a7c0_6ddb),
    (0x3ff4_9539_f0f0_10b0, 0xbfd0_1eae_7f51_3a67),
    (0x3ff3_c995_b0b8_0385, 0xbfcb_31d8_a682_24e9),
    (0x3ff3_0d19_0c88_64a5, 0xbfc6_574f_0ac0_7758),
    (0x3ff2_5e22_7b0b_8ea0, 0xbfc1_aa2b_c79c_8100),
    (0x3ff1_bb4a_4a1a_343f, 0xbfba_4e76_ce8c_0e5e),
    (0x3ff1_2358_f08a_e5ba, 0xbfb1_973c_5a61_1ccc),
    (0x3ff0_953f_4199_00a7, 0xbfa2_52f4_38e1_0c1e),
    (0x3ff0_0000_0000_0000, 0x0000_0000_0000_0000),
    (0x3fee_608c_fd9a_47ac, 0x3faa_a5aa_5df2_5984),
    (0x3fec_a4b3_1f02_6aa0, 0x3fbc_5e53_aa36_2eb4),
    (0x3feb_2036_576a_fce6, 0x3fc5_26e5_7720_db08),
    (0x3fe9_c2d1_63a1_aa2d, 0x3fcb_c286_0d22_4770),
    (0x3fe8_86e6_0378_41ed, 0x3fd1_058b_c8a0_7ee1),
    (0x3fe7_67dc_f553_4862, 0x3fd4_0430_57b6_ee09),
];
/// ln 2 en double précision.
const LN2: u64 = 0x3fe6_2e42_fefa_39ef;
/// Polynôme d'approximation de log1p(r) - r (coefficients de r⁴, r³, r²).
const POLY: [u64; 3] = [
    0xbfd0_0ea3_48b8_8334,
    0x3fd5_575b_0be0_0b6a,
    0xbfdf_fffe_f20a_4123,
];
/// Début du premier sous-intervalle (bits de ≈ 0,6992).
const OFF: u32 = 0x3f33_0000;

/// Logarithme népérien, identique au bit près sur toutes les plateformes.
///
/// Mêmes cas particuliers que `f32::ln` : ln 1 = 0, ln 0 = -∞, ln +∞ = +∞,
/// NaN pour un argument négatif ou NaN.
///
/// ```
/// assert_eq!(game_core::math::ln(1.0), 0.0);
/// assert_eq!(game_core::math::ln(2.0).to_bits(), 0x3f31_7218);
/// ```
#[must_use]
pub fn ln(x: f32) -> f32 {
    let mut ix = x.to_bits();
    if ix == 0x3f80_0000 {
        return 0.0;
    }
    if ix.wrapping_sub(0x0080_0000) >= 0x7f80_0000 - 0x0080_0000 {
        // x < 2^-126 (dénormal, nul ou négatif), infini ou NaN.
        if ix.wrapping_mul(2) == 0 {
            return f32::NEG_INFINITY;
        }
        if ix == 0x7f80_0000 {
            return x;
        }
        if ix & 0x8000_0000 != 0 || ix.wrapping_mul(2) >= 0xff00_0000 {
            return f32::NAN;
        }
        // Dénormal : normalisation exacte (multiplication par 2^23).
        ix = (x * f32::from_bits(0x4b00_0000)).to_bits();
        ix = ix.wrapping_sub(23 << 23);
    }
    // x = 2^k·z, z dans [OFF, 2·OFF] (exact) ; c proche du centre du
    // sous-intervalle i contenant z ; ln x = log1p(z/c - 1) + ln c + k·ln 2.
    let tmp = ix.wrapping_sub(OFF);
    let i = ((tmp >> 19) % 16) as usize;
    let k = (tmp as i32) >> 23;
    let iz = ix.wrapping_sub(tmp & 0xff80_0000);
    let invc = f64::from_bits(TABLE[i].0);
    let logc = f64::from_bits(TABLE[i].1);
    let z = f64::from(f32::from_bits(iz));
    // Mêmes opérations, dans le même ordre, que la glibc ; Rust ne contracte
    // jamais a·b + c en FMA, et les deux variantes de la glibc donnent ici le
    // même résultat (vérifié sur tous les f32).
    let r = z * invc - 1.0;
    let y0 = logc + f64::from(k) * f64::from_bits(LN2);
    let r2 = r * r;
    let y = f64::from_bits(POLY[1]) * r + f64::from_bits(POLY[2]);
    let y = f64::from_bits(POLY[0]) * r2 + y;
    let y = y * r2 + (y0 + r);
    y as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (x, ln x) en bits, calculés par `logf` de la glibc 2.39 : ces valeurs
    /// ne dépendent pas de la plateforme qui exécute le test.
    const REFERENCE: [(u32, u32); 13] = [
        (0x4000_0000, 0x3f31_7218), // 2
        (0x4040_0000, 0x3f8c_9f54), // 3
        (0x4120_0000, 0x4013_5d8e), // 10
        (0x447a_0000, 0x40dd_0c55), // 1000
        (0x4780_0080, 0x4131_7228), // 65537
        (0x4f00_0000, 0x41ab_e687), // 2^31
        (0x4f7f_ffff, 0x41b1_7218), // 2^32 - 256
        (0x3f00_0000, 0xbf31_7218), // 0,5
        (0x0da2_4260, 0xc28a_27b5), // 1e-30
        (0x0000_0001, 0xc2ce_8ed0), // plus petit dénormal
        (0x007f_ffff, 0xc2ae_ac50), // plus grand dénormal
        (0x7f7f_ffff, 0x42b1_7218), // f32::MAX
        (0x3f80_0001, 0x33ff_ffff), // 1 + 2^-23
    ];

    #[test]
    fn matches_reference_values_bit_for_bit() {
        for (x, expected) in REFERENCE {
            let got = ln(f32::from_bits(x)).to_bits();
            assert_eq!(got, expected, "ln({}) = {got:#x}", f32::from_bits(x));
        }
    }

    #[test]
    fn special_cases() {
        assert_eq!(ln(1.0).to_bits(), 0);
        assert_eq!(ln(0.0), f32::NEG_INFINITY);
        assert_eq!(ln(-0.0), f32::NEG_INFINITY);
        assert_eq!(ln(f32::INFINITY), f32::INFINITY);
        assert!(ln(-1.0).is_nan());
        assert!(ln(f32::NAN).is_nan());
        assert!(ln(f32::NEG_INFINITY).is_nan());
    }

    #[test]
    fn close_to_the_exact_logarithm() {
        // Écart à ln calculé en double précision : moins d'un ULP f32.
        let mut v = 1u32;
        while v < 5_000_000 {
            let x = v as f32;
            let exact = f64::from(x).ln();
            let ulp =
                f64::from(f32::from_bits((exact as f32).to_bits() + 1)) - f64::from(exact as f32);
            assert!((f64::from(ln(x)) - exact).abs() <= ulp.abs(), "{v}");
            v = v * 3 / 2 + 1;
        }
    }

    /// Comparaison exhaustive avec la bibliothèque du système (glibc ≥ 2.28
    /// attendue) : `cargo test -p game-core --release -- --ignored`. Sous
    /// macOS ou Windows, des écarts sont possibles : c'est précisément ce que
    /// ce module évite au moteur.
    #[test]
    #[ignore = "long (2·10⁹ valeurs) ; n'a de sens qu'avec la glibc"]
    fn identical_to_glibc_logf_on_every_positive_float() {
        let mut mismatches = 0u64;
        for bits in 1u32..0x7f80_0000 {
            let x = f32::from_bits(bits);
            if ln(x).to_bits() != x.ln().to_bits() {
                mismatches += 1;
            }
        }
        assert_eq!(mismatches, 0);
    }
}
