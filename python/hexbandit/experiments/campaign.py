"""Campagnes d'expériences.

Une campagne est décrite en TOML (lu par :mod:`tomllib`) : tailles, ratios de
budget r = 2^k, budgets de base B0, algorithmes, nombre de parties par
cellule, graine maître. Le joueur A (budget r·B0) commence toujours ; sa
couleur alterne d'une partie à l'autre. Chaque cellule est exécutée par
tranches enregistrées dès qu'elles sont terminées : une campagne interrompue
reprend là où elle s'était arrêtée.
"""

from __future__ import annotations

import fnmatch
import hashlib
import json
import math
import time
import tomllib
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import UTC, datetime
from pathlib import Path
from typing import TYPE_CHECKING, Any, Literal

from pydantic import Field, model_validator

from hexbandit.domain.errors import Cancelled, ConfigError
from hexbandit.domain.types import (
    BetaSpec,
    FinalMove,
    GameRecord,
    GameSpec,
    MatchSpec,
    PlayerSpec,
    PriorSpec,
    Seating,
    _Model,
)

if TYPE_CHECKING:
    from hexbandit.domain.ports import EnginePort, ResultsRepository


class AlgorithmTemplate(_Model):
    """Réglages d'un algorithme, sans budget (fixé par la cellule)."""

    algorithm: Literal["uct", "rave", "random"]
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

    def player(self, simulations: int) -> PlayerSpec:
        """Joueur avec le budget indiqué (simulations par coup)."""
        return PlayerSpec(simulations=simulations, **self.model_dump(exclude_none=True))


class CampaignSettings(_Model):
    """Paramètres généraux."""

    name: str
    master_seed: int = Field(ge=0, lt=2**63)
    games_per_cell: int = Field(ge=2)
    chunk_size: int = Field(default=50, ge=1)
    threads: int = Field(default=2, ge=1)
    max_cost_simulations: float = Field(default=5e11, gt=0)
    fill_factor: float = Field(default=0.6, gt=0.0, le=1.0)
    throughput: float = Field(default=150_000.0, gt=0.0)
    seating: Seating = "a_first_alternate_colors"
    kind: Literal["hex", "reverse_hex", "chameleon"] = "hex"
    swap: bool = False


class PlanSettings(_Model):
    """Plan factoriel."""

    sizes: list[int] = Field(min_length=1)
    ratio_exponents: list[int] = Field(min_length=1)
    base_budgets: list[int] = Field(min_length=1)
    algorithms: list[str] = Field(min_length=1)


class Reduction(_Model):
    """Réduction du plan (si le coût dépasse le plafond).

    Portée : les tailles ``sizes`` et, si précisés, les budgets de base
    ``base_budgets`` et les ``algorithms``. Effet : nombre de parties
    ``games_per_cell``, sous-ensemble de ``ratio_exponents``, ou exclusion
    des cellules (``skip``). La première réduction dont la portée correspond s'applique.
    """

    sizes: list[int] = Field(min_length=1)
    base_budgets: list[int] | None = None
    algorithms: list[str] | None = None
    games_per_cell: int | None = Field(default=None, ge=2)
    ratio_exponents: list[int] | None = None
    skip: bool = False

    def applies(self, algorithm: str, base_budget: int, size: int) -> bool:
        """Vrai si la cellule est dans la portée de la réduction."""
        return (
            size in self.sizes
            and (self.base_budgets is None or base_budget in self.base_budgets)
            and (self.algorithms is None or algorithm in self.algorithms)
        )


@dataclass(frozen=True)
class Cell:
    """Cellule du plan : (algorithme, B0, N, k), avec r = 2^k."""

    algorithm: str
    base_budget: int
    size: int
    ratio_exponent: int
    games: int
    master_seed: int

    @property
    def cell_id(self) -> str:
        """Identifiant stable, utilisé pour les fichiers et les graines."""
        return f"{self.algorithm}_B{self.base_budget}_N{self.size:02d}_k{self.ratio_exponent:+d}"

    @property
    def ratio(self) -> float:
        """Rapport de budget r = B_premier / B_second."""
        return float(2.0**self.ratio_exponent)

    @property
    def budget_first(self) -> int:
        """Budget du premier joueur (A)."""
        return round(self.base_budget * self.ratio)

    @property
    def budget_second(self) -> int:
        """Budget du second joueur (B), égal à B0."""
        return self.base_budget

    def expected_simulations(self, fill_factor: float) -> float:
        """Coût attendu (simulations) : ``fill_factor`` × N² coups par partie."""
        moves = fill_factor * self.size * self.size
        return self.games * moves / 2.0 * (self.budget_first + self.budget_second)

    def max_simulations(self) -> float:
        """Coût maximal (plateau entièrement rempli)."""
        return self.expected_simulations(1.0)


def _cell_seed(master_seed: int, cell_id: str) -> int:
    digest = hashlib.blake2b(f"{master_seed}:{cell_id}".encode(), digest_size=8).digest()
    return int.from_bytes(digest, "little") & ((1 << 63) - 1)


class CampaignConfig(_Model):
    """Description complète d'une campagne (fichier TOML)."""

    campaign: CampaignSettings
    plan: PlanSettings
    players: dict[str, AlgorithmTemplate]
    reductions: list[Reduction] = Field(default_factory=list)

    @model_validator(mode="after")
    def _check(self) -> CampaignConfig:
        for name in self.plan.algorithms:
            if name not in self.players:
                raise ValueError(f"algorithme {name!r} absent de la table [players]")
        if self.campaign.games_per_cell % 2:
            raise ValueError("games_per_cell doit être pair (alternance des couleurs)")
        for b0 in self.plan.base_budgets:
            for k in self.plan.ratio_exponents:
                if k < 0 and b0 % (2 ** (-k)):
                    raise ValueError(f"budget non entier : B0={b0}, r=2^{k}")
        for size in self.plan.sizes:
            if not 2 <= size <= 19:
                raise ValueError(f"taille {size} hors de [2, 19]")
        for reduction in self.reductions:
            if reduction.games_per_cell is not None and reduction.games_per_cell % 2:
                raise ValueError("games_per_cell doit être pair dans les réductions")
        return self

    @staticmethod
    def load(path: str | Path) -> CampaignConfig:
        """Lit et valide un fichier TOML."""
        try:
            with Path(path).open("rb") as handle:
                data = tomllib.load(handle)
            return CampaignConfig.model_validate(data)
        except (OSError, tomllib.TOMLDecodeError, ValueError) as exc:
            raise ConfigError(f"campagne {path} invalide : {exc}") from exc

    def config_hash(self) -> str:
        """Empreinte de la configuration (détecte une reprise incohérente)."""
        canonical = json.dumps(self.model_dump(mode="json"), sort_keys=True)
        return hashlib.sha256(canonical.encode()).hexdigest()[:16]

    def _reduction_for(self, algorithm: str, base_budget: int, size: int) -> Reduction | None:
        for reduction in self.reductions:
            if reduction.applies(algorithm, base_budget, size):
                return reduction
        return None

    def cells(self) -> list[Cell]:
        """Cellules du plan, réductions appliquées, groupées par (algorithme, B0, N)
        et ordonnées des groupes les moins coûteux aux plus coûteux."""
        out = []
        for algorithm in self.plan.algorithms:
            for b0 in self.plan.base_budgets:
                for size in self.plan.sizes:
                    reduction = self._reduction_for(algorithm, b0, size)
                    if reduction is not None and reduction.skip:
                        continue
                    games = self.campaign.games_per_cell
                    exponents = self.plan.ratio_exponents
                    if reduction is not None:
                        games = reduction.games_per_cell or games
                        exponents = reduction.ratio_exponents or exponents
                    for k in exponents:
                        cell_id = f"{algorithm}_B{b0}_N{size:02d}_k{k:+d}"
                        out.append(
                            Cell(
                                algorithm=algorithm,
                                base_budget=b0,
                                size=size,
                                ratio_exponent=k,
                                games=games,
                                master_seed=_cell_seed(self.campaign.master_seed, cell_id),
                            )
                        )
        group_cost: dict[tuple[str, int, int], float] = {}
        for c in out:
            key = (c.algorithm, c.base_budget, c.size)
            group_cost[key] = group_cost.get(key, 0.0) + c.max_simulations()
        return sorted(
            out,
            key=lambda c: (
                group_cost[(c.algorithm, c.base_budget, c.size)],
                c.algorithm,
                c.ratio_exponent,
            ),
        )

    def match_spec(self, cell: Cell, offset: int, n_games: int) -> MatchSpec:
        """Match d'une tranche de cellule : A (r·B0) contre B (B0)."""
        template = self.players[cell.algorithm]
        return MatchSpec(
            game=GameSpec(kind=self.campaign.kind, size=cell.size, swap=self.campaign.swap),
            a=template.player(cell.budget_first),
            b=template.player(cell.budget_second),
            n_games=n_games,
            offset=offset,
            master_seed=cell.master_seed,
            seating=self.campaign.seating,
        )


@dataclass(frozen=True)
class CostEstimate:
    """Estimation du coût d'une campagne."""

    cells: int
    games: int
    expected_simulations: float
    max_simulations: float
    by_size: dict[int, float]
    eta_seconds: float

    def describe(self) -> str:
        """Résumé lisible."""
        lines = [
            f"{self.cells} cellules, {self.games} parties",
            f"coût attendu : {self.expected_simulations:.3e} simulations "
            f"(maximum {self.max_simulations:.3e})",
            f"durée estimée : {self.eta_seconds / 3600:.2f} h",
        ]
        for size, cost in sorted(self.by_size.items()):
            lines.append(f"  N={size:2d} : {cost:.3e} simulations")
        return "\n".join(lines)


def estimate_cost(config: CampaignConfig) -> CostEstimate:
    """Estime le coût total en simulations et la durée."""
    settings = config.campaign
    cells = config.cells()
    by_size: dict[int, float] = {}
    for cell in cells:
        by_size[cell.size] = by_size.get(cell.size, 0.0) + cell.expected_simulations(
            settings.fill_factor
        )
    expected = sum(by_size.values())
    return CostEstimate(
        cells=len(cells),
        games=sum(c.games for c in cells),
        expected_simulations=expected,
        max_simulations=sum(c.max_simulations() for c in cells),
        by_size=by_size,
        eta_seconds=expected / (settings.throughput * settings.threads),
    )


@dataclass(frozen=True)
class ChunkResult:
    """Tranche de parties d'une cellule, prête à être enregistrée."""

    cell: Cell
    offset: int
    rows: list[dict[str, Any]]


def rows_from_records(
    config: CampaignConfig, cell: Cell, records: list[GameRecord], metadata: dict[str, Any]
) -> list[dict[str, Any]]:
    """Une ligne par partie, avec configuration, versions et hash Git."""
    rows = []
    for record in records:
        rows.append(
            {
                "campaign": config.campaign.name,
                "cell_id": cell.cell_id,
                "algorithm": cell.algorithm,
                "base_budget": cell.base_budget,
                "size": cell.size,
                "ratio_exponent": cell.ratio_exponent,
                "ratio": cell.ratio,
                "budget_first": cell.budget_first,
                "budget_second": cell.budget_second,
                "game_index": record.match_index,
                "seed": str(record.seed),
                "first_color": record.first_player,
                "winner": record.winner,
                "first_won": int(record.winner == record.first_player),
                "num_moves": record.num_moves,
                "duration_ms": round(record.duration_ms, 3),
                "engine_version": record.engine_version,
                "hexbandit_version": metadata.get("hexbandit_version", "?"),
                "git_hash": metadata.get("git_hash", "?"),
                "config_hash": metadata.get("config_hash", "?"),
                "timestamp": datetime.now(UTC).isoformat(timespec="seconds"),
            }
        )
    return rows


def _duration(seconds: float) -> str:
    """Durée lisible (``1 h 05``, ``12 min``, ``40 s``)."""
    if math.isnan(seconds) or seconds < 0:
        return "?"
    if seconds >= 3600:
        return f"{int(seconds // 3600)} h {int(seconds % 3600 // 60):02d}"
    if seconds >= 60:
        return f"{round(seconds / 60)} min"
    return f"{round(seconds)} s"


@dataclass(frozen=True)
class ChunkSelection:
    """Sous-ensemble des tranches d'une campagne (exécution partielle ou répartie).

    * ``cells`` : motifs ``fnmatch`` sur l'identifiant de cellule
      (``"*_B2048_N14_*"``) ; vide = toutes les cellules ;
    * ``games_limit`` : seules les tranches commençant avant cette partie
      (``200`` = les 200 premières parties de chaque cellule) ;
    * ``shard`` : ``(i, n)`` avec 1 ≤ i ≤ n : une tranche sur n, à partir de
      la i-ème, pour répartir la campagne entre n machines (7.8).

    Les tranches sélectionnées sont exactement celles du plan complet : leurs
    parties ne dépendent que de la graine maître et de l'identifiant de cellule.
    """

    cells: tuple[str, ...] = ()
    games_limit: int | None = None
    shard: tuple[int, int] | None = None

    def __post_init__(self) -> None:
        if self.shard is not None:
            i, n = self.shard
            if not 1 <= i <= n:
                raise ConfigError(f"part {i}/{n} invalide (1 ≤ i ≤ n)")
        if self.games_limit is not None and self.games_limit < 1:
            raise ConfigError("games_limit doit être ≥ 1")

    def accepts_cell(self, cell: Cell) -> bool:
        """Vrai si la cellule correspond à l'un des motifs (ou s'il n'y en a pas)."""
        return not self.cells or any(fnmatch.fnmatchcase(cell.cell_id, p) for p in self.cells)

    def accepts_index(self, index: int) -> bool:
        """Vrai si la tranche de rang ``index`` (à partir de 0) revient à cette part."""
        return self.shard is None or index % self.shard[1] == self.shard[0] - 1


@dataclass
class RunSummary:
    """Bilan d'une exécution de campagne."""

    chunks_done: int = 0
    chunks_skipped: int = 0
    games_played: int = 0
    seconds: float = 0.0
    complete: bool = False
    stopped_by_time_limit: bool = False
    messages: list[str] = field(default_factory=list)


class CampaignRunner:
    """Exécution cellule par cellule, avec sauvegarde incrémentale et reprise."""

    def __init__(
        self,
        config: CampaignConfig,
        engine: EnginePort,
        repository: ResultsRepository,
        metadata: dict[str, Any],
        progress: Callable[[str], None] | None = None,
    ) -> None:
        self.config = config
        self.engine = engine
        self.repository = repository
        self.metadata = dict(metadata)
        self.metadata.setdefault("config_hash", config.config_hash())
        self.progress = progress or (lambda message: None)

    def check_budget(self, force: bool = False) -> CostEstimate:
        """Refuse la campagne si le coût attendu dépasse le plafond."""
        estimate = estimate_cost(self.config)
        ceiling = self.config.campaign.max_cost_simulations
        if estimate.expected_simulations > ceiling and not force:
            raise ConfigError(
                f"coût estimé {estimate.expected_simulations:.3e} simulations supérieur au plafond "
                f"{ceiling:.3e} : réduire le plan (section [[reductions]]) ou relever le plafond"
            )
        return estimate

    def chunks(self, selection: ChunkSelection | None = None) -> list[tuple[Cell, int]]:
        """Tranches (cellule, première partie) du plan, dans l'ordre d'exécution,
        restreintes par ``selection`` (filtre de cellules, plafond, répartition)."""
        selection = selection or ChunkSelection()
        size = self.config.campaign.chunk_size
        out: list[tuple[Cell, int]] = []
        index = 0
        for cell in self.config.cells():
            if not selection.accepts_cell(cell):
                continue
            for offset in range(0, cell.games, size):
                if selection.games_limit is not None and offset >= selection.games_limit:
                    break
                if selection.accepts_index(index):
                    out.append((cell, offset))
                index += 1
        return out

    def run(
        self,
        time_limit: float | None = None,
        force: bool = False,
        selection: ChunkSelection | None = None,
    ) -> RunSummary:
        """Exécute les tranches manquantes ; s'arrête proprement après ``time_limit`` s.

        ``selection`` restreint les tranches jouées (motif de cellules, plafond
        de parties par cellule, part ``i/n`` pour répartir une campagne entre
        machines) ; les tranches restent identiques à celles du plan complet.
        """
        self.check_budget(force)
        summary = RunSummary()
        start = time.monotonic()
        settings = self.config.campaign
        planned = self.chunks(selection)
        order: dict[str, int] = {}
        for cell, _ in planned:
            order.setdefault(cell.cell_id, len(order) + 1)

        def chunk_cost(cell: Cell, offset: int) -> float:
            n_games = min(settings.chunk_size, cell.games - offset)
            return cell.expected_simulations(settings.fill_factor) * n_games / cell.games

        todo = [(c, o) for c, o in planned if not self.repository.has_chunk(c.cell_id, o)]
        summary.chunks_skipped = len(planned) - len(todo)
        remaining = sum(chunk_cost(c, o) for c, o in todo)
        done_cost = 0.0
        for cell, offset in todo:
            elapsed = time.monotonic() - start
            if time_limit is not None and elapsed > time_limit:
                summary.stopped_by_time_limit = True
                summary.seconds = elapsed
                self.progress(f"limite de temps atteinte après {elapsed:.0f} s")
                return summary
            n_games = min(settings.chunk_size, cell.games - offset)
            spec = self.config.match_spec(cell, offset, n_games)
            t0 = time.monotonic()
            try:
                records = self.engine.play_match(spec, settings.threads)
            except Cancelled:
                summary.seconds = time.monotonic() - start
                self.progress("campagne interrompue ; relancer pour reprendre")
                raise
            rows = rows_from_records(self.config, cell, records, self.metadata)
            self.repository.save(ChunkResult(cell=cell, offset=offset, rows=rows))
            summary.chunks_done += 1
            summary.games_played += len(rows)
            cost = chunk_cost(cell, offset)
            done_cost += cost
            remaining -= cost
            spent = time.monotonic() - start
            eta = remaining * spent / done_cost if done_cost > 0 else float("nan")
            wins = sum(r["first_won"] for r in rows)
            self.progress(
                f"[{order[cell.cell_id]}/{len(order)}] {cell.cell_id} parties "
                f"{offset}-{offset + n_games - 1} : premier joueur {wins}/{n_games} "
                f"({time.monotonic() - t0:.1f} s) · {summary.chunks_done}/{len(todo)} tranches, "
                f"reste ≈ {_duration(eta)}"
            )
        summary.complete = True
        summary.seconds = time.monotonic() - start
        return summary
