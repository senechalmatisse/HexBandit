//! Configurations versionnées, validées côté Rust : source de vérité unique.
//! Les champs inconnus sont refusés, les valeurs hors bornes aussi ;
//! les valeurs par défaut sont appliquées ici et renvoyées normalisées dans les enregistrements.

use game_core::Player;
use game_hex::{MAX_SIZE, MIN_SIZE, notation::player_name};
use mcts::runner::Seating;

use crate::{error::BindingError, json::Json};

/// Version du schéma de configuration.
pub const SCHEMA_VERSION: u64 = 1;
/// Biais RAVE b par défaut, réglé empiriquement.
pub const DEFAULT_RAVE_BIAS: f32 = 0.02;
/// Paramètre d'équivalence k par défaut ; l'article trouve k ≥ 1000 optimal.
pub const DEFAULT_HAND_K: f32 = 1000.0;
/// Limite de nœuds par défaut.
pub const DEFAULT_MAX_NODES: u32 = 250_000;

type Result<T> = std::result::Result<T, BindingError>;

/// Lecteur des champs d'un objet JSON : signale les champs inconnus.
pub(crate) struct Fields<'a> {
    path: String,
    fields: &'a [(String, Json)],
    used: Vec<bool>,
}

impl<'a> Fields<'a> {
    pub(crate) fn new(value: &'a Json, path: &str) -> Result<Self> {
        match value {
            Json::Object(fields) => Ok(Fields {
                path: path.to_owned(),
                fields,
                used: vec![false; fields.len()],
            }),
            other => Err(BindingError::config(format!(
                "{path} : objet attendu, {} reçu",
                other.type_name()
            ))),
        }
    }

    fn at(&self, key: &str) -> String {
        if self.path.is_empty() {
            key.to_owned()
        } else {
            format!("{}.{key}", self.path)
        }
    }

    pub(crate) fn take(&mut self, key: &str) -> Option<&'a Json> {
        let index = self.fields.iter().position(|(k, _)| k == key)?;
        self.used[index] = true;
        match &self.fields[index].1 {
            Json::Null => None,
            v => Some(v),
        }
    }

    pub(crate) fn opt_u64(&mut self, key: &str) -> Result<Option<u64>> {
        match self.take(key) {
            None => Ok(None),
            Some(Json::Number(text)) => text.parse::<u64>().map(Some).map_err(|_| {
                BindingError::config(format!(
                    "{} : entier positif attendu, {text} reçu",
                    self.at(key)
                ))
            }),
            Some(other) => Err(self.type_error(key, "entier", other)),
        }
    }

    pub(crate) fn req_u64(&mut self, key: &str) -> Result<u64> {
        self.opt_u64(key)?
            .ok_or_else(|| BindingError::config(format!("{} : champ obligatoire", self.at(key))))
    }

    pub(crate) fn opt_f32(&mut self, key: &str) -> Result<Option<f32>> {
        match self.take(key) {
            None => Ok(None),
            Some(Json::Number(text)) => match text.parse::<f64>() {
                Ok(v) if v.is_finite() => Ok(Some(v as f32)),
                _ => Err(BindingError::config(format!(
                    "{} : réel invalide {text}",
                    self.at(key)
                ))),
            },
            Some(other) => Err(self.type_error(key, "réel", other)),
        }
    }

    pub(crate) fn opt_bool(&mut self, key: &str) -> Result<Option<bool>> {
        match self.take(key) {
            None => Ok(None),
            Some(Json::Bool(b)) => Ok(Some(*b)),
            Some(other) => Err(self.type_error(key, "booléen", other)),
        }
    }

    pub(crate) fn opt_str(&mut self, key: &str) -> Result<Option<&'a str>> {
        match self.take(key) {
            None => Ok(None),
            Some(Json::String(s)) => Ok(Some(s.as_str())),
            Some(other) => Err(self.type_error(key, "chaîne", other)),
        }
    }

    pub(crate) fn opt_strings(&mut self, key: &str) -> Result<Vec<String>> {
        match self.take(key) {
            None => Ok(Vec::new()),
            Some(Json::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Json::String(s) => Ok(s.clone()),
                    other => Err(self.type_error(key, "tableau de chaînes", other)),
                })
                .collect(),
            Some(other) => Err(self.type_error(key, "tableau", other)),
        }
    }

    fn type_error(&self, key: &str, expected: &str, got: &Json) -> BindingError {
        BindingError::config(format!(
            "{} : {expected} attendu, {} reçu",
            self.at(key),
            got.type_name()
        ))
    }

    pub(crate) fn path_of(&self, key: &str) -> String {
        self.at(key)
    }

    /// Termine la lecture : erreur si un champ n'a pas été consommé.
    pub(crate) fn finish(self) -> Result<()> {
        let unknown: Vec<&str> = self
            .fields
            .iter()
            .zip(&self.used)
            .filter(|(_, used)| !**used)
            .map(|((k, _), _)| k.as_str())
            .collect();
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(BindingError::config(format!(
                "{} : champ(s) inconnu(s) {}",
                if self.path.is_empty() {
                    "racine"
                } else {
                    &self.path
                },
                unknown.join(", ")
            )))
        }
    }
}

fn check_schema(fields: &mut Fields<'_>) -> Result<()> {
    let version = fields.opt_u64("schema_version")?.unwrap_or(SCHEMA_VERSION);
    if version != SCHEMA_VERSION {
        return Err(BindingError::config(format!(
            "schema_version {version} non pris en charge (attendu {SCHEMA_VERSION})"
        )));
    }
    Ok(())
}

fn parse_player_color(text: &str, path: &str) -> Result<Player> {
    game_hex::notation::parse_player(text)
        .map_err(|_| BindingError::config(format!("{path} : couleur inconnue {text:?}")))
}

// ---------------------------------------------------------------------------
// Jeu
// ---------------------------------------------------------------------------

/// Variante de jeu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameKind {
    /// Hex.
    Hex,
    /// Reverse Hex (relier ses bords fait perdre).
    ReverseHex,
    /// Quarto (alignements seulement).
    Quarto,
    /// Quarto, variante avancée (les carrés 2 × 2 comptent).
    QuartoSquares,
    /// Chameleon : chaque joueur pose des pierres des deux couleurs.
    Chameleon,
}

impl GameKind {
    /// Nom stable.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            GameKind::Hex => "hex",
            GameKind::ReverseHex => "reverse_hex",
            GameKind::Quarto => "quarto",
            GameKind::QuartoSquares => "quarto_squares",
            GameKind::Chameleon => "chameleon",
        }
    }
}

/// Description du jeu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameSpec {
    /// Variante.
    pub kind: GameKind,
    /// Taille du plateau.
    pub size: usize,
    /// Règle du gâteau.
    pub swap: bool,
}

impl GameSpec {
    pub(crate) fn parse(value: &Json, path: &str) -> Result<Self> {
        let mut f = Fields::new(value, path)?;
        let kind = match f.opt_str("kind")?.unwrap_or("hex") {
            "hex" => GameKind::Hex,
            "reverse_hex" | "reverse-hex" => GameKind::ReverseHex,
            "quarto" => GameKind::Quarto,
            "quarto_squares" => GameKind::QuartoSquares,
            "chameleon" => GameKind::Chameleon,
            other => {
                return Err(BindingError::config(format!(
                    "{} : jeu inconnu {other:?}",
                    f.path_of("kind")
                )));
            }
        };
        let quarto = matches!(kind, GameKind::Quarto | GameKind::QuartoSquares);
        let default_size = if quarto {
            4
        } else {
            game_hex::DEFAULT_SIZE as u64
        };
        let size = f.opt_u64("size")?.unwrap_or(default_size) as usize;
        if quarto && size != 4 {
            return Err(BindingError::config(format!(
                "{} : le Quarto se joue sur 4 × 4",
                f.path_of("size")
            )));
        }
        if !(MIN_SIZE..=MAX_SIZE).contains(&size) {
            return Err(BindingError::config(format!(
                "{} : {size} hors de [{MIN_SIZE}, {MAX_SIZE}]",
                f.path_of("size")
            )));
        }
        let swap = f.opt_bool("swap")?.unwrap_or(false);
        if quarto && swap {
            return Err(BindingError::config(format!(
                "{} : pas de règle du gâteau au Quarto",
                f.path_of("swap")
            )));
        }
        f.finish()?;
        Ok(GameSpec { kind, size, swap })
    }

    /// Représentation JSON normalisée.
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::object(vec![
            ("kind", Json::str(self.kind.as_str())),
            ("size", Json::int(self.size as u64)),
            ("swap", Json::Bool(self.swap)),
        ])
    }
}

// ---------------------------------------------------------------------------
// Joueur
// ---------------------------------------------------------------------------

/// Algorithme d'un joueur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlgorithmSpec {
    /// MCTS/UCT.
    Uct,
    /// MC-RAVE.
    Rave,
    /// Coups uniformes (référence).
    Random,
}

/// Budget par coup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetSpec {
    /// Simulations par coup (déterministe).
    Simulations(u64),
    /// Millisecondes par coup (non déterministe).
    TimeMs(u64),
}

/// Planning de β.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BetaSpec {
    /// Éq. 19.
    HandSelected {
        /// Paramètre d'équivalence.
        k: f32,
    },
    /// Éq. 47.
    MinimumMse {
        /// Biais RAVE.
        bias: f32,
    },
    /// β = 0.
    None,
}

/// Politique de simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RolloutSpec {
    /// Remplissage aléatoire et une vérification.
    Fill,
    /// Coups uniformes un par un.
    Uniform,
}

/// Stratégie de choix final.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FinalMoveSpec {
    /// Plus visité.
    MostVisited,
    /// Meilleure valeur.
    BestValue,
    /// Borne inférieure de confiance Q - a/√N la plus haute.
    Secure {
        /// Coefficient a.
        a: f32,
    },
    /// Coup à la fois le plus visité et le mieux évalué, sinon le plus visité
    MaxRobust,
}

impl FinalMoveSpec {
    fn as_str(self) -> &'static str {
        match self {
            FinalMoveSpec::MostVisited => "most_visited",
            FinalMoveSpec::BestValue => "best_value",
            FinalMoveSpec::Secure { .. } => "secure",
            FinalMoveSpec::MaxRobust => "max_robust",
        }
    }
}

/// Coefficient par défaut du choix « secure child ».
pub const DEFAULT_SECURE_A: f32 = 1.0;

/// Connaissance a priori.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriorSpec {
    /// Heuristique « partie équilibrée » de l'article (Q = 0,5, confiance M).
    Even {
        /// Confiance M.
        confidence: u32,
    },
    /// Motifs du Hex : confiances, en simulations équivalentes,
    /// données aux cases mortes (valeur 0), aux coups qui sauvent un pont
    /// (1) ou en coupent un (0,9), qui créent un pont ou un pont vers le bord
    /// (0,75), et aux autres coups (0,5).
    Hex(HexPriorWeights),
}

/// Poids de la connaissance a priori du Hex.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HexPriorWeights {
    /// Cases mortes.
    pub dead: u32,
    /// Sauvetage d'un pont du joueur.
    pub save: u32,
    /// Coupure d'un pont adverse.
    pub cut: u32,
    /// Nouveau pont ou pont vers le bord.
    pub bridge: u32,
    /// Autres coups (0 : aucune connaissance).
    pub even: u32,
}

impl Default for HexPriorWeights {
    fn default() -> Self {
        HexPriorWeights {
            dead: 40,
            save: 30,
            cut: 20,
            bridge: 10,
            even: 0,
        }
    }
}

/// Configuration complète d'un joueur.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerSpec {
    /// Algorithme.
    pub algorithm: AlgorithmSpec,
    /// Budget (ignoré pour `random`).
    pub budget: BudgetSpec,
    /// Constante d'exploration c.
    pub exploration: f32,
    /// Planning de β (RAVE).
    pub beta: BetaSpec,
    /// Politique de simulation.
    pub rollout: RolloutSpec,
    /// Choix final.
    pub final_move: FinalMoveSpec,
    /// Seuil d'expansion.
    pub expand_threshold: u32,
    /// Limite de nœuds.
    pub max_nodes: u32,
    /// Connaissance a priori.
    pub prior: Option<PriorSpec>,
    /// MCTS-Solver.
    pub solver: bool,
    /// Réutilisation du sous-arbre du coup joué.
    pub reuse_tree: bool,
    /// Seuil GRAVE (0 = MC-RAVE).
    pub grave_ref: u32,
    /// Nombre d'arbres de la recherche parallèle à la racine (1 = recherche séquentielle).
    pub trees: u32,
    /// Nom libre.
    pub label: Option<String>,
}

impl PlayerSpec {
    pub(crate) fn parse(value: &Json, path: &str) -> Result<Self> {
        let mut f = Fields::new(value, path)?;
        let algorithm = match f.opt_str("algorithm")? {
            Some("uct") => AlgorithmSpec::Uct,
            Some("rave" | "mc-rave" | "mc_rave") => AlgorithmSpec::Rave,
            Some("random") => AlgorithmSpec::Random,
            Some(other) => {
                return Err(BindingError::config(format!(
                    "{} : algorithme inconnu {other:?}",
                    f.path_of("algorithm")
                )));
            }
            None => {
                return Err(BindingError::config(format!(
                    "{} : champ obligatoire",
                    f.path_of("algorithm")
                )));
            }
        };
        let sims = f.opt_u64("simulations")?;
        let time = f.opt_u64("time_ms")?;
        let budget = match (sims, time, algorithm) {
            (Some(_), Some(_), _) => {
                return Err(BindingError::config(format!(
                    "{path} : simulations et time_ms sont exclusifs"
                )));
            }
            (Some(0), _, _) | (_, Some(0), _) => {
                return Err(BindingError::config(format!(
                    "{path} : le budget doit être ≥ 1"
                )));
            }
            (Some(s), None, _) => BudgetSpec::Simulations(s),
            (None, Some(t), _) => BudgetSpec::TimeMs(t),
            (None, None, AlgorithmSpec::Random) => BudgetSpec::Simulations(1),
            (None, None, _) => {
                return Err(BindingError::config(format!(
                    "{path} : budget manquant (simulations ou time_ms)"
                )));
            }
        };
        let default_c = match algorithm {
            AlgorithmSpec::Uct => std::f32::consts::SQRT_2,
            _ => 0.0,
        };
        let exploration = f.opt_f32("exploration")?.unwrap_or(default_c);
        if exploration < 0.0 {
            return Err(BindingError::config(format!(
                "{} : doit être ≥ 0",
                f.path_of("exploration")
            )));
        }
        let beta = match f.take("beta") {
            None => BetaSpec::MinimumMse {
                bias: DEFAULT_RAVE_BIAS,
            },
            Some(v) => parse_beta(v, &f.path_of("beta"))?,
        };
        let rollout = match f.opt_str("rollout")?.unwrap_or("fill") {
            "fill" => RolloutSpec::Fill,
            "uniform" => RolloutSpec::Uniform,
            other => {
                return Err(BindingError::config(format!(
                    "{} : politique inconnue {other:?}",
                    f.path_of("rollout")
                )));
            }
        };
        let secure_a = f.opt_f32("secure_a")?;
        let final_move = match f.opt_str("final_move")?.unwrap_or("most_visited") {
            "most_visited" => FinalMoveSpec::MostVisited,
            "best_value" => FinalMoveSpec::BestValue,
            "secure" | "secure_child" => FinalMoveSpec::Secure {
                a: secure_a.unwrap_or(DEFAULT_SECURE_A),
            },
            "max_robust" => FinalMoveSpec::MaxRobust,
            other => {
                return Err(BindingError::config(format!(
                    "{} : stratégie inconnue {other:?}",
                    f.path_of("final_move")
                )));
            }
        };
        if let FinalMoveSpec::Secure { a } = final_move {
            if a < 0.0 {
                return Err(BindingError::config(format!(
                    "{} : doit être ≥ 0",
                    f.path_of("secure_a")
                )));
            }
        } else if secure_a.is_some() {
            return Err(BindingError::config(format!(
                "{} : réservé à final_move = \"secure\"",
                f.path_of("secure_a")
            )));
        }
        let expand_threshold = bounded_u32(&mut f, "expand_threshold", 1, 1)?;
        let max_nodes = bounded_u32(&mut f, "max_nodes", DEFAULT_MAX_NODES, 1)?;
        let prior = match f.take("prior") {
            None => None,
            Some(v) => Some(parse_prior(v, &f.path_of("prior"))?),
        };
        let solver = f.opt_bool("solver")?.unwrap_or(false);
        let reuse_tree = f.opt_bool("reuse_tree")?.unwrap_or(false);
        let grave_ref = bounded_u32(&mut f, "grave_ref", 0, 0)?;
        if grave_ref > 0 && algorithm != AlgorithmSpec::Rave {
            return Err(BindingError::config(format!(
                "{} : GRAVE ne s'applique qu'à rave",
                f.path_of("grave_ref")
            )));
        }
        let trees = bounded_u32(&mut f, "trees", 1, 1)?;
        if trees > 256 {
            return Err(BindingError::config(format!(
                "{} : au plus 256 arbres",
                f.path_of("trees")
            )));
        }
        let label = f.opt_str("label")?.map(str::to_owned);
        f.finish()?;
        Ok(PlayerSpec {
            algorithm,
            budget,
            exploration,
            beta,
            rollout,
            final_move,
            expand_threshold,
            max_nodes,
            prior,
            solver,
            reuse_tree,
            grave_ref,
            trees,
            label,
        })
    }

    /// Nom court lisible, par exemple `rave-2048`.
    #[must_use]
    pub fn short_label(&self) -> String {
        if let Some(label) = &self.label {
            return label.clone();
        }
        let algo = match self.algorithm {
            AlgorithmSpec::Uct => "uct",
            AlgorithmSpec::Rave => "rave",
            AlgorithmSpec::Random => return "random".to_owned(),
        };
        match self.budget {
            BudgetSpec::Simulations(s) => format!("{algo}-{s}"),
            BudgetSpec::TimeMs(t) => format!("{algo}-{t}ms"),
        }
    }

    /// Représentation JSON normalisée (toutes les valeurs effectives).
    #[must_use]
    pub fn to_json(&self) -> Json {
        let mut fields = vec![(
            "algorithm",
            Json::str(match self.algorithm {
                AlgorithmSpec::Uct => "uct",
                AlgorithmSpec::Rave => "rave",
                AlgorithmSpec::Random => "random",
            }),
        )];
        if self.algorithm != AlgorithmSpec::Random {
            match self.budget {
                BudgetSpec::Simulations(s) => fields.push(("simulations", Json::int(s))),
                BudgetSpec::TimeMs(t) => fields.push(("time_ms", Json::int(t))),
            }
            fields.push(("exploration", Json::float32(self.exploration)));
            if self.algorithm == AlgorithmSpec::Rave {
                fields.push(("beta", beta_to_json(self.beta)));
            }
            fields.push((
                "rollout",
                Json::str(match self.rollout {
                    RolloutSpec::Fill => "fill",
                    RolloutSpec::Uniform => "uniform",
                }),
            ));
            fields.push(("final_move", Json::str(self.final_move.as_str())));
            if let FinalMoveSpec::Secure { a } = self.final_move {
                fields.push(("secure_a", Json::float32(a)));
            }
            fields.push(("expand_threshold", Json::int(self.expand_threshold)));
            fields.push(("max_nodes", Json::int(self.max_nodes)));
            if let Some(prior) = self.prior {
                fields.push(("prior", prior_to_json(prior)));
            }
            if self.solver {
                fields.push(("solver", Json::Bool(true)));
            }
            if self.reuse_tree {
                fields.push(("reuse_tree", Json::Bool(true)));
            }
            if self.grave_ref > 0 {
                fields.push(("grave_ref", Json::int(self.grave_ref)));
            }
            if self.trees > 1 {
                fields.push(("trees", Json::int(self.trees)));
            }
        }
        fields.push(("label", Json::str(self.short_label())));
        Json::object(fields)
    }
}

fn bounded_u32(f: &mut Fields<'_>, key: &str, default: u32, min: u32) -> Result<u32> {
    let value = f.opt_u64(key)?.unwrap_or(u64::from(default));
    let value = u32::try_from(value)
        .map_err(|_| BindingError::config(format!("{} : trop grand", f.path_of(key))))?;
    if value < min {
        return Err(BindingError::config(format!(
            "{} : doit être ≥ {min}",
            f.path_of(key)
        )));
    }
    Ok(value)
}

fn parse_prior(value: &Json, path: &str) -> Result<PriorSpec> {
    let mut p = Fields::new(value, path)?;
    let spec = match p.opt_str("kind")?.unwrap_or("even") {
        "even" => {
            let confidence = p.req_u64("confidence")?;
            PriorSpec::Even {
                confidence: u32::try_from(confidence)
                    .map_err(|_| BindingError::config(format!("{path}.confidence : trop grand")))?,
            }
        }
        "hex" => {
            let d = HexPriorWeights::default();
            PriorSpec::Hex(HexPriorWeights {
                dead: bounded_u32(&mut p, "dead", d.dead, 0)?,
                save: bounded_u32(&mut p, "save", d.save, 0)?,
                cut: bounded_u32(&mut p, "cut", d.cut, 0)?,
                bridge: bounded_u32(&mut p, "bridge", d.bridge, 0)?,
                even: bounded_u32(&mut p, "even", d.even, 0)?,
            })
        }
        other => {
            return Err(BindingError::config(format!(
                "{} : prior inconnu {other:?}",
                p.path_of("kind")
            )));
        }
    };
    p.finish()?;
    Ok(spec)
}

fn prior_to_json(prior: PriorSpec) -> Json {
    match prior {
        PriorSpec::Even { confidence } => Json::object(vec![
            ("kind", Json::str("even")),
            ("confidence", Json::int(confidence)),
        ]),
        PriorSpec::Hex(w) => Json::object(vec![
            ("kind", Json::str("hex")),
            ("dead", Json::int(w.dead)),
            ("save", Json::int(w.save)),
            ("cut", Json::int(w.cut)),
            ("bridge", Json::int(w.bridge)),
            ("even", Json::int(w.even)),
        ]),
    }
}

fn parse_beta(value: &Json, path: &str) -> Result<BetaSpec> {
    let mut f = Fields::new(value, path)?;
    let spec = match f.opt_str("schedule")?.unwrap_or("mse") {
        "mse" | "minimum_mse" => {
            let bias = f.opt_f32("bias")?.unwrap_or(DEFAULT_RAVE_BIAS);
            if bias < 0.0 {
                return Err(BindingError::config(format!("{path}.bias : doit être ≥ 0")));
            }
            BetaSpec::MinimumMse { bias }
        }
        "manual" | "hand_selected" => {
            let k = f.opt_f32("k")?.unwrap_or(DEFAULT_HAND_K);
            if k <= 0.0 {
                return Err(BindingError::config(format!("{path}.k : doit être > 0")));
            }
            BetaSpec::HandSelected { k }
        }
        "none" => BetaSpec::None,
        other => {
            return Err(BindingError::config(format!(
                "{path}.schedule : planning inconnu {other:?}"
            )));
        }
    };
    f.finish()?;
    Ok(spec)
}

fn beta_to_json(beta: BetaSpec) -> Json {
    match beta {
        BetaSpec::HandSelected { k } => Json::object(vec![
            ("schedule", Json::str("manual")),
            ("k", Json::float32(k)),
        ]),
        BetaSpec::MinimumMse { bias } => Json::object(vec![
            ("schedule", Json::str("mse")),
            ("bias", Json::float32(bias)),
        ]),
        BetaSpec::None => Json::object(vec![("schedule", Json::str("none"))]),
    }
}

// ---------------------------------------------------------------------------
// Parties, matchs, recherches
// ---------------------------------------------------------------------------

/// Niveau de détail des rapports dans l'enregistrement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReportLevel {
    /// Aucun rapport (campagnes).
    None,
    /// Compteurs globaux par coup.
    Summary,
    /// Compteurs et statistiques de chaque candidat (cartes de chaleur).
    Full,
}

impl ReportLevel {
    fn parse(text: &str, path: &str) -> Result<Self> {
        match text {
            "none" => Ok(ReportLevel::None),
            "summary" => Ok(ReportLevel::Summary),
            "full" => Ok(ReportLevel::Full),
            other => Err(BindingError::config(format!(
                "{path} : niveau inconnu {other:?}"
            ))),
        }
    }

    /// Nom stable.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ReportLevel::None => "none",
            ReportLevel::Summary => "summary",
            ReportLevel::Full => "full",
        }
    }
}

/// Information de match attachée à une partie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchInfo {
    /// Indice global de la partie dans le match.
    pub index: u64,
    /// Couleur jouée par A.
    pub a_color: Player,
}

/// Configuration d'une partie (play_game).
#[derive(Clone, Debug, PartialEq)]
pub struct GameConfig {
    /// Jeu.
    pub game: GameSpec,
    /// Premier joueur.
    pub first_player: Player,
    /// Joueurs par couleur : `[bleu, rouge]`.
    pub players: [PlayerSpec; 2],
    /// Graine de la partie.
    pub seed: u64,
    /// Niveau de détail des rapports.
    pub report: ReportLevel,
    /// Coups d'ouverture imposés.
    pub opening: Vec<String>,
    /// Nombre maximal de coups.
    pub max_plies: u32,
    /// Information de match, le cas échéant.
    pub match_info: Option<MatchInfo>,
}

fn default_max_plies(game: &GameSpec) -> u32 {
    (2 * game.size * game.size + 10) as u32
}

impl GameConfig {
    /// Lit une configuration de partie.
    ///
    /// # Errors
    ///
    /// [`BindingError`] de type `config` si le document est invalide.
    pub fn parse(value: &Json, path: &str) -> Result<Self> {
        let mut f = Fields::new(value, path)?;
        check_schema(&mut f)?;
        let game = GameSpec::parse(
            f.take("game").unwrap_or(&Json::Object(Vec::new())),
            &f.path_of("game"),
        )?;
        let first_player = match f.opt_str("first_player")? {
            None => Player::P1,
            Some(text) => parse_player_color(text, &f.path_of("first_player"))?,
        };
        let players_value = f.take("players").ok_or_else(|| {
            BindingError::config(format!("{} : champ obligatoire", f.path_of("players")))
        })?;
        let mut p = Fields::new(players_value, &f.path_of("players"))?;
        let blue = PlayerSpec::parse(
            p.take("blue").ok_or_else(|| {
                BindingError::config(format!("{} : champ obligatoire", p.path_of("blue")))
            })?,
            &p.path_of("blue"),
        )?;
        let red = PlayerSpec::parse(
            p.take("red").ok_or_else(|| {
                BindingError::config(format!("{} : champ obligatoire", p.path_of("red")))
            })?,
            &p.path_of("red"),
        )?;
        p.finish()?;
        let seed = f.opt_u64("seed")?.unwrap_or(0);
        let report = match f.opt_str("report")? {
            None => ReportLevel::Summary,
            Some(text) => ReportLevel::parse(text, &f.path_of("report"))?,
        };
        let opening = f.opt_strings("opening")?;
        let max_plies = match f.opt_u64("max_plies")? {
            None => default_max_plies(&game),
            Some(v) => {
                u32::try_from(v).map_err(|_| BindingError::config("max_plies trop grand"))?
            }
        };
        f.finish()?;
        Ok(GameConfig {
            game,
            first_player,
            players: [blue, red],
            seed,
            report,
            opening,
            max_plies,
            match_info: None,
        })
    }

    /// Joueur de la couleur indiquée.
    #[must_use]
    pub fn player(&self, color: Player) -> &PlayerSpec {
        &self.players[color.index()]
    }
}

/// Match de n parties entre A et B.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchSpec {
    /// Jeu.
    pub game: GameSpec,
    /// Joueur A.
    pub a: PlayerSpec,
    /// Joueur B.
    pub b: PlayerSpec,
    /// Nombre de parties de cette tranche.
    pub n_games: u32,
    /// Indice de la première partie.
    pub offset: u64,
    /// Graine maître.
    pub master_seed: u64,
    /// Placement.
    pub seating: Seating,
    /// Niveau de détail des rapports.
    pub report: ReportLevel,
}

impl MatchSpec {
    pub(crate) fn parse(value: &Json, path: &str) -> Result<Self> {
        let mut f = Fields::new(value, path)?;
        check_schema(&mut f)?;
        let game = GameSpec::parse(
            f.take("game").unwrap_or(&Json::Object(Vec::new())),
            &f.path_of("game"),
        )?;
        let a = PlayerSpec::parse(
            f.take("a").ok_or_else(|| {
                BindingError::config(format!("{} : champ obligatoire", f.path_of("a")))
            })?,
            &f.path_of("a"),
        )?;
        let b = PlayerSpec::parse(
            f.take("b").ok_or_else(|| {
                BindingError::config(format!("{} : champ obligatoire", f.path_of("b")))
            })?,
            &f.path_of("b"),
        )?;
        let n_games = f.req_u64("n_games")?;
        let n_games =
            u32::try_from(n_games).map_err(|_| BindingError::config("n_games trop grand"))?;
        if n_games == 0 {
            return Err(BindingError::config(format!(
                "{} : doit être ≥ 1",
                f.path_of("n_games")
            )));
        }
        let offset = f.opt_u64("offset")?.unwrap_or(0);
        let master_seed = f.req_u64("master_seed")?;
        let seating = match f.opt_str("seating")?.unwrap_or("alternate_first") {
            "alternate_first" => Seating::AlternateFirst,
            "a_first_alternate_colors" => Seating::AFirstAlternateColors,
            other => {
                return Err(BindingError::config(format!(
                    "{} : placement inconnu {other:?}",
                    f.path_of("seating")
                )));
            }
        };
        let report = match f.opt_str("report")? {
            None => ReportLevel::None,
            Some(text) => ReportLevel::parse(text, &f.path_of("report"))?,
        };
        f.finish()?;
        Ok(MatchSpec {
            game,
            a,
            b,
            n_games,
            offset,
            master_seed,
            seating,
            report,
        })
    }

    /// Configurations des parties du match, avec graines dérivées et alternance.
    #[must_use]
    pub fn games(&self) -> Vec<GameConfig> {
        let plan = mcts::runner::MatchPlan {
            master_seed: self.master_seed,
            n_games: self.n_games,
            offset: self.offset,
            seating: self.seating,
        };
        plan.seats()
            .map(|seat| {
                let mut players = [self.a.clone(), self.b.clone()];
                if seat.a_player == Player::P2 {
                    players.swap(0, 1);
                }
                GameConfig {
                    game: self.game,
                    first_player: seat.first_player,
                    players,
                    seed: seat.seed,
                    report: self.report,
                    opening: Vec::new(),
                    max_plies: default_max_plies(&self.game),
                    match_info: Some(MatchInfo {
                        index: seat.index,
                        a_color: seat.a_player,
                    }),
                }
            })
            .collect()
    }
}

/// Requête de recherche sur une position (search).
#[derive(Clone, Debug, PartialEq)]
pub struct SearchRequest {
    /// Jeu.
    pub game: GameSpec,
    /// Premier joueur.
    pub first_player: Player,
    /// Coups menant à la position.
    pub moves: Vec<String>,
    /// Position textuelle (alternative à `moves`).
    pub position: Option<String>,
    /// Joueur qui cherche.
    pub player: PlayerSpec,
    /// Graine.
    pub seed: u64,
}

impl SearchRequest {
    pub(crate) fn parse(value: &Json) -> Result<Self> {
        let mut f = Fields::new(value, "")?;
        check_schema(&mut f)?;
        let game = GameSpec::parse(f.take("game").unwrap_or(&Json::Object(Vec::new())), "game")?;
        let first_player = match f.opt_str("first_player")? {
            None => Player::P1,
            Some(text) => parse_player_color(text, "first_player")?,
        };
        let moves = f.opt_strings("moves")?;
        let position = f.opt_str("position")?.map(str::to_owned);
        if position.is_some() && !moves.is_empty() {
            return Err(BindingError::config("moves et position sont exclusifs"));
        }
        let player = PlayerSpec::parse(
            f.take("player")
                .ok_or_else(|| BindingError::config("player : champ obligatoire"))?,
            "player",
        )?;
        let seed = f.opt_u64("seed")?.unwrap_or(0);
        f.finish()?;
        Ok(SearchRequest {
            game,
            first_player,
            moves,
            position,
            player,
            seed,
        })
    }
}

/// Nom de couleur pour les enregistrements.
#[must_use]
pub fn color_name(player: Player) -> &'static str {
    player_name(player)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_player(text: &str) -> Result<PlayerSpec> {
        PlayerSpec::parse(&Json::parse(text).unwrap(), "p")
    }

    #[test]
    fn player_defaults_and_validation() {
        let p = parse_player(r#"{"algorithm":"rave","simulations":100}"#).unwrap();
        assert_eq!(
            p.beta,
            BetaSpec::MinimumMse {
                bias: DEFAULT_RAVE_BIAS
            }
        );
        assert_eq!(p.exploration, 0.0);
        assert_eq!(p.rollout, RolloutSpec::Fill);
        let p = parse_player(r#"{"algorithm":"uct","time_ms":5}"#).unwrap();
        assert_eq!(p.budget, BudgetSpec::TimeMs(5));
        assert!((p.exploration - std::f32::consts::SQRT_2).abs() < 1e-6);
        for bad in [
            r#"{"algorithm":"uct"}"#,
            r#"{"algorithm":"uct","simulations":0}"#,
            r#"{"algorithm":"uct","simulations":10,"time_ms":5}"#,
            r#"{"algorithm":"alphazero","simulations":10}"#,
            r#"{"algorithm":"uct","simulations":10,"colour":"blue"}"#,
            r#"{"algorithm":"uct","simulations":-3}"#,
            r#"{"algorithm":"uct","simulations":1.5}"#,
            r#"{"algorithm":"rave","simulations":10,"beta":{"schedule":"manual","k":0}}"#,
            r#"{"algorithm":"uct","simulations":10,"exploration":-1}"#,
            r#"{"algorithm":"uct","simulations":10,"grave_ref":10}"#,
            r#"{"algorithm":"rave","simulations":10,"trees":0}"#,
            r#"{"algorithm":"rave","simulations":10,"prior":{"kind":"hex","oops":1}}"#,
            r#"{"algorithm":"rave","simulations":10,"secure_a":1.0}"#,
            r#"{"algorithm":"rave","simulations":10,"solver":1}"#,
        ] {
            assert!(parse_player(bad).is_err(), "{bad}");
        }
        let plain = parse_player(r#"{"algorithm":"rave","simulations":64}"#).unwrap();
        let text = plain.to_json().to_string();
        for key in ["solver", "reuse_tree", "grave_ref", "trees", "secure_a"] {
            assert!(!text.contains(key), "{text}");
        }
        let v2 = parse_player(
            r#"{"algorithm":"rave","simulations":64,"solver":true,"reuse_tree":true,
                "grave_ref":40,"trees":2,"final_move":"secure","prior":{"kind":"hex","dead":7}}"#,
        )
        .unwrap();
        let labelled = PlayerSpec {
            label: Some(v2.short_label()),
            ..v2.clone()
        };
        assert_eq!(PlayerSpec::parse(&v2.to_json(), "p").unwrap(), labelled);
        assert_eq!(
            v2.prior,
            Some(PriorSpec::Hex(HexPriorWeights {
                dead: 7,
                ..HexPriorWeights::default()
            }))
        );
        let normalized =
            parse_player(r#"{"algorithm":"rave","simulations":64,"beta":{"schedule":"manual"}}"#)
                .unwrap()
                .to_json();
        let again = PlayerSpec::parse(&normalized, "p").unwrap();
        assert_eq!(again.beta, BetaSpec::HandSelected { k: DEFAULT_HAND_K });
    }

    #[test]
    fn match_alternates_colors() {
        let spec = MatchSpec::parse(
            &Json::parse(
                r#"{"game":{"size":5},"a":{"algorithm":"uct","simulations":10},
                    "b":{"algorithm":"rave","simulations":10},"n_games":4,"master_seed":9,
                    "seating":"a_first_alternate_colors"}"#,
            )
            .unwrap(),
            "match",
        )
        .unwrap();
        let games = spec.games();
        assert_eq!(games.len(), 4);
        for g in &games {
            let info = g.match_info.unwrap();
            assert_eq!(g.first_player, info.a_color);
            assert_eq!(g.player(info.a_color).algorithm, AlgorithmSpec::Uct);
        }
        assert_ne!(games[0].seed, games[1].seed);
    }
}
