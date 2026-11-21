"""Types du domaine : configurations (validées par pydantic) et
enregistrements de parties (format ``hexbandit/game-record``).

Les configurations sont aussi validées par le moteur Rust, source de vérité
du schéma ; la validation pydantic sert à refuser tôt une campagne mal écrite.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator

from hexbandit.domain.errors import RecordFormatError

Color = Literal["blue", "red"]
Winner = Literal["blue", "red", "draw"]
ReportLevel = Literal["none", "summary", "full"]
Seating = Literal["alternate_first", "a_first_alternate_colors"]
Proven = Literal["win", "loss", "draw"]

RECORD_FORMAT = "hexbandit/game-record"
RECORD_FORMAT_VERSION = 1
SCHEMA_VERSION = 1


def other_color(color: Color) -> Color:
    """Couleur adverse."""
    return "red" if color == "blue" else "blue"


class _Model(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)


class BetaSpec(_Model):
    """Planning de β de MC-RAVE."""

    schedule: Literal["mse", "manual", "none"] = "mse"
    bias: float | None = Field(default=None, ge=0.0)
    k: float | None = Field(default=None, gt=0.0)


FinalMove = Literal["most_visited", "best_value", "secure", "max_robust"]


class PriorSpec(_Model):
    """Connaissance a priori.

    * ``even`` : heuristique « partie équilibrée » (Q = 0,5, ``confidence``) ;
    * ``hex`` : motifs du Hex, confiances données aux cases mortes
      (``dead``), aux coups qui sauvent (``save``) ou coupent (``cut``) un pont,
      qui créent un pont ou un pont vers le bord (``bridge``) et aux autres
      coups (``even``) ; valeurs par défaut du moteur si omises.
    """

    kind: Literal["even", "hex"] = "even"
    confidence: int | None = Field(default=None, ge=0)
    dead: int | None = Field(default=None, ge=0)
    save: int | None = Field(default=None, ge=0)
    cut: int | None = Field(default=None, ge=0)
    bridge: int | None = Field(default=None, ge=0)
    even: int | None = Field(default=None, ge=0)

    @model_validator(mode="after")
    def _check_kind(self) -> PriorSpec:
        hex_fields = (self.dead, self.save, self.cut, self.bridge, self.even)
        if self.kind == "even":
            if self.confidence is None:
                raise ValueError("prior even : confidence obligatoire")
            if any(v is not None for v in hex_fields):
                raise ValueError("prior even : seul confidence est accepté")
        elif self.confidence is not None:
            raise ValueError("prior hex : utiliser dead, save, cut, bridge, even")
        return self


class PlayerSpec(_Model):
    """Configuration d'un joueur (algorithme, budget, réglages).

    Options (désactivées par défaut) : ``solver`` (MCTS-Solver),
    ``reuse_tree`` (réutilisation du sous-arbre), ``grave_ref``
    (GRAVE, MC-RAVE seulement), ``trees`` (parallélisme à la racine),
    ``prior`` de type ``hex`` et les choix finaux ``secure`` et ``max_robust``.
    """

    algorithm: Literal["uct", "rave", "random"]
    simulations: int | None = Field(default=None, ge=1)
    time_ms: int | None = Field(default=None, ge=1)
    exploration: float | None = Field(default=None, ge=0.0)
    beta: BetaSpec | None = None
    rollout: Literal["fill", "uniform"] | None = None
    final_move: FinalMove | None = None
    secure_a: float | None = Field(default=None, ge=0.0)
    expand_threshold: int | None = Field(default=None, ge=1)
    max_nodes: int | None = Field(default=None, ge=1)
    prior: PriorSpec | None = None
    solver: bool | None = None
    reuse_tree: bool | None = None
    grave_ref: int | None = Field(default=None, ge=0)
    trees: int | None = Field(default=None, ge=1, le=256)
    label: str | None = None

    @model_validator(mode="after")
    def _check_budget(self) -> PlayerSpec:
        if self.simulations is not None and self.time_ms is not None:
            raise ValueError("simulations et time_ms sont exclusifs")
        if self.algorithm != "random" and self.simulations is None and self.time_ms is None:
            raise ValueError("budget manquant : simulations ou time_ms")
        if self.grave_ref and self.algorithm != "rave":
            raise ValueError("grave_ref ne s'applique qu'à rave")
        if self.secure_a is not None and self.final_move != "secure":
            raise ValueError('secure_a est réservé à final_move = "secure"')
        return self

    def with_simulations(self, simulations: int) -> PlayerSpec:
        """Copie avec un autre budget en simulations par coup."""
        return self.model_copy(update={"simulations": simulations, "time_ms": None})

    def to_engine(self) -> dict[str, Any]:
        """Dictionnaire pour le moteur (champs non renseignés omis)."""
        return self.model_dump(exclude_none=True)


GameKind = Literal["hex", "reverse_hex", "quarto", "quarto_squares", "chameleon"]


class GameSpec(_Model):
    """Jeu : variante, taille, règle du gâteau.

    ``quarto`` et ``quarto_squares`` (carrés 2 × 2 gagnants) se jouent sur
    4 × 4, sans règle du gâteau, et seulement avec UCT (pas d'AMAF).
    ``chameleon`` se joue sur le plateau du Hex, chaque joueur posant des
    pierres des deux couleurs (coups ``c4b``, ``c4r``).
    """

    kind: GameKind = "hex"
    size: int = Field(default=14, ge=2, le=19)
    swap: bool = False

    @model_validator(mode="before")
    @classmethod
    def _quarto_defaults(cls, data: Any) -> Any:
        if isinstance(data, dict) and str(data.get("kind", "")).startswith("quarto"):
            data = {**data}
            data.setdefault("size", 4)
        return data

    @model_validator(mode="after")
    def _check_quarto(self) -> GameSpec:
        if self.is_quarto and (self.size != 4 or self.swap):
            raise ValueError("le Quarto se joue sur 4 × 4, sans règle du gâteau")
        return self

    @property
    def is_quarto(self) -> bool:
        """Vrai pour les variantes de Quarto."""
        return self.kind.startswith("quarto")


class GameConfig(_Model):
    """Configuration d'une partie."""

    game: GameSpec = GameSpec()
    first_player: Color = "blue"
    blue: PlayerSpec
    red: PlayerSpec
    seed: int = Field(default=0, ge=0, lt=2**64)
    report: ReportLevel = "summary"
    opening: tuple[str, ...] = ()

    def to_engine(self) -> dict[str, Any]:
        """Requête ``play_game`` pour le moteur."""
        return {
            "schema_version": SCHEMA_VERSION,
            "game": self.game.model_dump(),
            "first_player": self.first_player,
            "players": {"blue": self.blue.to_engine(), "red": self.red.to_engine()},
            "seed": self.seed,
            "report": self.report,
            "opening": list(self.opening),
        }


class MatchSpec(_Model):
    """Match de n parties entre A et B, graines dérivées d'une graine maître."""

    game: GameSpec = GameSpec()
    a: PlayerSpec
    b: PlayerSpec
    n_games: int = Field(ge=1)
    offset: int = Field(default=0, ge=0)
    master_seed: int = Field(ge=0, lt=2**64)
    seating: Seating = "alternate_first"
    report: ReportLevel = "none"

    def to_engine(self) -> dict[str, Any]:
        """Requête ``play_batch`` (forme ``match``) pour le moteur."""
        return {
            "schema_version": SCHEMA_VERSION,
            "match": {
                "game": self.game.model_dump(),
                "a": self.a.to_engine(),
                "b": self.b.to_engine(),
                "n_games": self.n_games,
                "offset": self.offset,
                "master_seed": self.master_seed,
                "seating": self.seating,
                "report": self.report,
            },
        }


class SearchRequest(_Model):
    """Recherche sur une position (coups depuis le début, ou position textuelle)."""

    game: GameSpec = GameSpec()
    first_player: Color = "blue"
    moves: tuple[str, ...] = ()
    position: str | None = None
    player: PlayerSpec
    seed: int = Field(default=0, ge=0, lt=2**64)

    def to_engine(self) -> dict[str, Any]:
        """Requête ``search`` pour le moteur."""
        payload: dict[str, Any] = {
            "schema_version": SCHEMA_VERSION,
            "game": self.game.model_dump(),
            "first_player": self.first_player,
            "player": self.player.to_engine(),
            "seed": self.seed,
        }
        if self.position is not None:
            payload["position"] = self.position
        else:
            payload["moves"] = list(self.moves)
        return payload


# ---------------------------------------------------------------------------
# Enregistrements
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CandidateStats:
    """Statistiques d'un coup candidat à la racine."""

    move: str
    n: int
    q: float
    n_amaf: int
    q_amaf: float
    value: float | None
    proven: Proven | None = None


@dataclass(frozen=True)
class SearchReport:
    """Rapport de recherche d'un coup."""

    simulations: int
    nodes: int
    max_depth: int
    mean_depth: float
    root_value: float
    best_visits: int
    best_value: float | None
    node_limit_reached: bool
    elapsed_ms: float
    candidates: tuple[CandidateStats, ...] = ()
    reused_visits: int = 0
    proven: Proven | None = None

    @staticmethod
    def from_json(data: dict[str, Any]) -> SearchReport:
        """Lecture depuis le JSON du moteur."""
        candidates = tuple(
            CandidateStats(
                move=str(c["move"]),
                n=int(c["n"]),
                q=float(c["q"]),
                n_amaf=int(c["n_amaf"]),
                q_amaf=float(c["q_amaf"]),
                value=None if c.get("value") is None else float(c["value"]),
                proven=_proven(c.get("proven")),
            )
            for c in data.get("candidates", [])
        )
        best_value = data.get("best_value")
        return SearchReport(
            simulations=int(data["simulations"]),
            nodes=int(data["nodes"]),
            max_depth=int(data["max_depth"]),
            mean_depth=float(data.get("mean_depth", 0.0)),
            root_value=float(data["root_value"]),
            best_visits=int(data["best_visits"]),
            best_value=None if best_value is None else float(best_value),
            node_limit_reached=bool(data.get("node_limit_reached", False)),
            elapsed_ms=float(data.get("elapsed_ms", 0.0)),
            candidates=candidates,
            reused_visits=int(data.get("reused_visits", 0)),
            proven=_proven(data.get("proven")),
        )


@dataclass(frozen=True)
class MoveRecord:
    """Coup enregistré."""

    ply: int
    player: Color
    move: str
    report: SearchReport | None = None


@dataclass(frozen=True)
class GameRecord:
    """Partie enregistrée : suffit à la rejouer et à l'afficher."""

    kind: str
    size: int
    swap: bool
    first_player: Color
    seed: int
    players: dict[str, dict[str, Any]]
    moves: tuple[MoveRecord, ...]
    winner: Winner | None
    duration_ms: float
    engine_version: str
    opening: tuple[str, ...] = ()
    match_index: int | None = None
    a_color: Color | None = None
    raw: dict[str, Any] = field(default_factory=dict, repr=False, compare=False)

    @property
    def num_moves(self) -> int:
        """Nombre de coups joués."""
        return len(self.moves)

    @staticmethod
    def from_json(data: dict[str, Any]) -> GameRecord:
        """Lecture et validation d'un enregistrement."""
        if data.get("format") != RECORD_FORMAT:
            raise RecordFormatError(f"format inconnu : {data.get('format')!r}")
        if data.get("format_version") != RECORD_FORMAT_VERSION:
            raise RecordFormatError(f"version de format inconnue : {data.get('format_version')!r}")
        try:
            game = data["game"]
            moves = tuple(
                MoveRecord(
                    ply=int(m["ply"]),
                    player=_color(m["player"]),
                    move=str(m["move"]),
                    report=SearchReport.from_json(m["report"]) if "report" in m else None,
                )
                for m in data["moves"]
            )
            winner = data.get("winner")
            match = data.get("match") or {}
            return GameRecord(
                kind=str(game["kind"]),
                size=int(game["size"]),
                swap=bool(game.get("swap", False)),
                first_player=_color(data["first_player"]),
                seed=int(data["seed"]),
                players={str(k): dict(v) for k, v in data["players"].items()},
                moves=moves,
                winner=None if winner is None else _winner(winner),
                duration_ms=float(data.get("duration_ms", 0.0)),
                engine_version=str(data.get("engine", {}).get("version", "?")),
                opening=tuple(str(m) for m in data.get("opening", [])),
                match_index=None if "index" not in match else int(match["index"]),
                a_color=None if "a_color" not in match else _color(match["a_color"]),
                raw=data,
            )
        except (KeyError, TypeError, ValueError) as exc:
            raise RecordFormatError(f"enregistrement invalide : {exc}") from exc


def _proven(value: Any) -> Proven | None:
    if value is None:
        return None
    if value in ("win", "loss", "draw"):
        result: Proven = value
        return result
    raise RecordFormatError(f"valeur prouvée inconnue : {value!r}")


def _color(value: Any) -> Color:
    if value == "blue":
        return "blue"
    if value == "red":
        return "red"
    raise RecordFormatError(f"couleur inconnue : {value!r}")


def _winner(value: Any) -> Winner:
    if value == "draw":
        return "draw"
    return _color(value)


@dataclass(frozen=True)
class SearchResult:
    """Résultat d'une recherche sur une position."""

    position: str
    player: Color
    best_move: str
    report: SearchReport
    player_config: dict[str, Any]

    @staticmethod
    def from_json(data: dict[str, Any]) -> SearchResult:
        """Lecture depuis le JSON du moteur."""
        return SearchResult(
            position=str(data["position"]),
            player=_color(data["player"]),
            best_move=str(data["best_move"]),
            report=SearchReport.from_json(data["report"]),
            player_config=dict(data.get("player_config", {})),
        )
