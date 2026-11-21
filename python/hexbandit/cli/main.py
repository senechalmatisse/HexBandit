"""Ligne de commande : ``play``, ``replay``, ``search``, ``match``,
``estimate``, ``run-campaign``, ``verify``, ``report``, ``adaptive`` (plan adaptatif de r*),
``tournament`` (tournoi et Elo) et ``view`` (visionneuse web).

Exemples ::

    hexbandit play --size 11 --blue rave:2000 --red uct:2000 --out partie.json --gif partie.gif
    hexbandit replay partie.json --out finale.png --heatmap 10
    hexbandit match --size 9 --a rave:3000 --b uct:3000 --games 200
    hexbandit run-campaign experiments/campagne_reduite.toml --results results/reduite
    hexbandit report results/reduite

Un joueur s'écrit ``algorithme:simulations[,option=valeur...]`` ; options :
``c`` (exploration), ``beta`` (mse, manual, none), ``bias``, ``k``,
``rollout`` (fill, uniform), ``final`` (most_visited, best_value, secure,
max_robust), ``time`` (ms par coup au lieu de simulations), ``nodes``,
``expand``, ``solver``, ``reuse``, ``grave``, ``trees``, ``prior`` (even:M, hex).
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from collections.abc import Sequence
from pathlib import Path
from typing import Any

from hexbandit import __version__, _native
from hexbandit._environment import python_label
from hexbandit.domain.errors import Cancelled, ConfigError, HexbanditError
from hexbandit.domain.hexboard import HexBoard
from hexbandit.domain.ports import EnginePort
from hexbandit.domain.types import (
    BetaSpec,
    Color,
    GameConfig,
    GameSpec,
    MatchSpec,
    PlayerSpec,
    PriorSpec,
    SearchRequest,
)

_FLAGS = {"solver": "solver", "reuse": "reuse_tree", "reuse_tree": "reuse_tree"}
_PRIOR_WEIGHTS = ("dead", "save", "cut", "bridge", "even")


def _flag(value: str) -> bool:
    if value in ("", "1", "true", "oui", "yes"):
        return True
    if value in ("0", "false", "non", "no"):
        return False
    raise ValueError(f"booléen attendu, reçu {value!r}")


def parse_player(text: str) -> PlayerSpec:
    """Lit ``rave:2000,beta=manual,k=1000`` en :class:`PlayerSpec`.

    Options : ``solver`` et ``reuse`` (drapeaux, ``=0`` pour les désactiver),
    ``grave=N``, ``trees=N``, ``prior=even:M`` ou ``prior=hex``
    (poids ``dead``, ``save``, ``cut``, ``bridge``, ``even`` réglables par
    ``prior.dead=40``…), ``final=secure`` (``secure_a=1.0``) ou ``final=max_robust``.
    """
    head, *options = [part.strip() for part in text.split(",") if part.strip()]
    algorithm, _, budget = head.partition(":")
    fields: dict[str, Any] = {"algorithm": algorithm.strip().lower()}
    if budget:
        try:
            fields["simulations"] = int(budget)
        except ValueError as exc:
            raise argparse.ArgumentTypeError(f"budget illisible dans {text!r}") from exc
    beta: dict[str, Any] = {}
    prior: dict[str, Any] = {}
    for option in options:
        key, _, value = option.partition("=")
        key = key.strip().lower()
        value = value.strip()
        try:
            if key in _FLAGS:
                fields[_FLAGS[key]] = _flag(value.lower())
                continue
            if key == "grave":
                fields["grave_ref"] = int(value)
                continue
            if key == "trees":
                fields["trees"] = int(value)
                continue
            if key == "secure_a":
                fields["secure_a"] = float(value)
                continue
            if key == "prior":
                kind, _, confidence = value.partition(":")
                prior["kind"] = kind.lower()
                if confidence:
                    prior["confidence"] = int(confidence)
                continue
            if key.startswith("prior.") and key[6:] in _PRIOR_WEIGHTS:
                prior.setdefault("kind", "hex")
                prior[key[6:]] = int(value)
                continue
        except ValueError as exc:
            raise argparse.ArgumentTypeError(f"option {key} invalide : {exc}") from exc
        if key == "c":
            fields["exploration"] = float(value)
        elif key == "beta":
            beta["schedule"] = value
        elif key == "bias":
            beta["bias"] = float(value)
        elif key == "k":
            beta["k"] = float(value)
        elif key == "rollout":
            fields["rollout"] = value
        elif key == "final":
            fields["final_move"] = value
        elif key == "time":
            fields.pop("simulations", None)
            fields["time_ms"] = int(value)
        elif key == "nodes":
            fields["max_nodes"] = int(value)
        elif key == "expand":
            fields["expand_threshold"] = int(value)
        elif key == "label":
            fields["label"] = value
        else:
            raise argparse.ArgumentTypeError(f"option de joueur inconnue : {key}")
    try:
        if beta:
            fields["beta"] = BetaSpec(**beta)
        if prior:
            fields["prior"] = PriorSpec(**prior)
        return PlayerSpec(**fields)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(f"joueur invalide {text!r} : {exc}") from exc


def _engine(fake: bool = False) -> EnginePort:
    if fake:
        from hexbandit.engine.fake import FakeEngine

        return FakeEngine()
    from hexbandit.engine.native import NativeEngine

    return NativeEngine()


def _game(args: argparse.Namespace) -> GameSpec:
    if str(args.kind).startswith("quarto"):
        return GameSpec(kind=args.kind, size=4, swap=False)
    return GameSpec(kind=args.kind, size=args.size, swap=args.swap)


def cmd_play(args: argparse.Namespace) -> int:
    """Joue une partie IA contre IA."""
    config = GameConfig(
        game=_game(args),
        first_player=args.first,
        blue=args.blue,
        red=args.red,
        seed=args.seed,
        report=args.report,
        opening=tuple(m for m in args.opening.split(",") if m) if args.opening else (),
    )
    start = time.monotonic()
    record = _engine().play_game(config)
    elapsed = time.monotonic() - start
    if config.game.is_quarto:
        print(" ".join(f"{m.ply}.{m.move}" for m in record.moves))
        print(f"{record.num_moves} coups en {elapsed:.2f} s ; résultat : {record.winner}")
        if args.out:
            Path(args.out).write_text(
                json.dumps(record.raw, indent=1, ensure_ascii=False), encoding="utf-8"
            )
        return 0
    board = HexBoard(record.size, record.first_player, chameleon=record.kind == "chameleon")
    board.play_all(record.opening)
    for move in record.moves:
        board.play(move.move, move.player)
    print(board.ascii())
    print(
        f"{record.num_moves} coups en {elapsed:.2f} s ; vainqueur : {record.winner} "
        f"(Bleu = {record.players['blue'].get('label')}, Rouge = {record.players['red'].get('label')})"
    )
    if args.out:
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(
            json.dumps(record.raw, indent=1, ensure_ascii=False), encoding="utf-8"
        )
        print(f"enregistrement : {args.out}")
    if args.image or args.gif:
        from hexbandit.viz.replay import MatplotlibRenderer

        renderer = MatplotlibRenderer()
        if args.image:
            print(f"image : {renderer.render_final(record, Path(args.image))}")
        if args.gif:
            print(f"animation : {renderer.render_animation(record, Path(args.gif))}")
    return 0


def cmd_replay(args: argparse.Namespace) -> int:
    """Rejoue un enregistrement : image finale, images, GIF/MP4, carte de chaleur."""
    from hexbandit.viz.replay import MatplotlibRenderer, load_record

    record = load_record(args.record)
    renderer = MatplotlibRenderer()
    base = Path(args.record).with_suffix("")
    outputs = []
    if args.out or not (args.frames or args.gif or args.mp4 or args.heatmap):
        outputs.append(renderer.render_final(record, Path(args.out or f"{base}_finale.png")))
    if args.frames:
        outputs += renderer.render_frames(record, Path(args.frames))[-1:]
    if args.gif:
        outputs.append(renderer.render_animation(record, Path(args.gif), fps=args.fps))
    if args.mp4:
        outputs.append(renderer.render_animation(record, Path(args.mp4), fps=args.fps))
    if args.heatmap:
        target = Path(args.heatmap_out or f"{base}_chaleur_{args.heatmap:03d}.png")
        outputs.append(renderer.render_heatmap(record, args.heatmap, target, metric=args.metric))
    for path in outputs:
        print(path)
    return 0


def cmd_view(args: argparse.Namespace) -> int:
    """Produit une page web autonome pour rejouer des enregistrements."""
    from hexbandit.viz.web import build_viewer

    records: list[dict[str, Any]] = []
    for name in args.records:
        data = json.loads(Path(name).read_text(encoding="utf-8"))
        if isinstance(data, dict) and "records" in data:
            data = data["records"]
        records.extend(data if isinstance(data, list) else [data])
    out = Path(args.out or Path(args.records[0]).with_suffix(".html"))
    print(build_viewer(records, out))
    return 0


def cmd_search(args: argparse.Namespace) -> int:
    """Analyse une position et affiche les meilleurs candidats."""
    request = SearchRequest(
        game=_game(args),
        first_player=args.first,
        moves=tuple(m for m in args.moves.split(",") if m) if args.moves else (),
        position=args.position,
        player=args.player,
        seed=args.seed,
    )
    result = _engine().search(request)
    print(
        f"position : {result.position} ; trait : {result.player} ; meilleur coup : {result.best_move}"
    )
    report = result.report
    extra = f" ; position prouvée : {PROVEN_NAMES[report.proven]}" if report.proven else ""
    print(
        f"{report.simulations} simulations, {report.nodes} nœuds, valeur racine {report.root_value:.3f}{extra}"
    )
    print("coup      N        Q      Ñ        Q̃     valeur  prouvé")
    for c in report.candidates[: args.top]:
        value = "-" if c.value is None else f"{c.value:.3f}"
        proven = PROVEN_NAMES[c.proven] if c.proven else ""
        print(
            f"{c.move:<6}{c.n:>6}  {c.q:7.3f}{c.n_amaf:>8}  {c.q_amaf:7.3f}  {value:>7}  {proven}"
        )
    return 0


PROVEN_NAMES = {"win": "gagnante", "loss": "perdante", "draw": "nulle"}


def cmd_match(args: argparse.Namespace) -> int:
    """Match A contre B ; affiche le taux de victoire de A avec IC et test binomial."""
    from hexbandit.experiments.stats import binomial_pvalue, wilson_interval

    spec = MatchSpec(
        game=_game(args),
        a=args.a,
        b=args.b,
        n_games=args.games,
        master_seed=args.seed,
        seating=args.seating,
    )
    start = time.monotonic()
    records = _engine().play_match(spec, args.threads)
    elapsed = time.monotonic() - start
    wins_a = sum(1 for r in records if r.winner == r.a_color)
    first_wins = sum(1 for r in records if r.winner == r.first_player)
    lo, hi = wilson_interval(wins_a, len(records))
    p_greater = binomial_pvalue(wins_a, len(records), alternative="greater")
    p_two = binomial_pvalue(wins_a, len(records))
    summary = {
        "size": args.size,
        "a": args.a.to_engine(),
        "b": args.b.to_engine(),
        "games": len(records),
        "wins_a": wins_a,
        "rate_a": wins_a / len(records),
        "ci_low": float(lo),
        "ci_high": float(hi),
        "p_value_greater": p_greater,
        "p_value_two_sided": p_two,
        "first_player_wins": first_wins,
        "seconds": elapsed,
        "seating": args.seating,
        "master_seed": args.seed,
    }
    print(
        f"A ({args.a.label or args.a.algorithm}) gagne {wins_a}/{len(records)} = {wins_a / len(records):.3f} "
        f"[IC 95 % {float(lo):.3f}, {float(hi):.3f}] ; p(A > 50 %) = {p_greater:.2e} ; "
        f"premier joueur : {first_wins}/{len(records)} ; {elapsed:.1f} s"
    )
    if args.out:
        out = Path(args.out)
        out.parent.mkdir(parents=True, exist_ok=True)
        existing = json.loads(out.read_text(encoding="utf-8")) if out.exists() else []
        existing.append(summary)
        out.write_text(json.dumps(existing, indent=1), encoding="utf-8")
    return 0


def cmd_estimate(args: argparse.Namespace) -> int:
    """Estime le coût d'une campagne."""
    from hexbandit.experiments.campaign import CampaignConfig, estimate_cost

    config = CampaignConfig.load(args.campaign)
    estimate = estimate_cost(config)
    print(estimate.describe())
    ceiling = config.campaign.max_cost_simulations
    verdict = "accepté" if estimate.expected_simulations <= ceiling else "REFUSÉ (plafond dépassé)"
    print(f"plafond : {ceiling:.3e} simulations -> {verdict}")
    return 0 if estimate.expected_simulations <= ceiling else 3


def _selection(args: argparse.Namespace) -> Any:
    """Sous-ensemble de tranches demandé (``--cells``, ``--games-limit``, ``--shard``)."""
    from hexbandit.experiments.campaign import ChunkSelection

    shard = None
    if getattr(args, "shard", None):
        first, _, total = str(args.shard).partition("/")
        try:
            shard = (int(first), int(total))
        except ValueError as exc:
            raise ConfigError(f"--shard attend i/n, reçu {args.shard!r}") from exc
    return ChunkSelection(
        cells=tuple(getattr(args, "cells", None) or ()),
        games_limit=getattr(args, "games_limit", None),
        shard=shard,
    )


def cmd_run_campaign(args: argparse.Namespace) -> int:
    """Exécute (ou reprend) une campagne."""
    from hexbandit.experiments.campaign import CampaignConfig, CampaignRunner
    from hexbandit.experiments.repository import FileResultsRepository, build_metadata

    config = CampaignConfig.load(args.campaign)
    if args.threads:
        config = config.model_copy(
            update={"campaign": config.campaign.model_copy(update={"threads": args.threads})}
        )
    engine = _engine(args.fake)
    repository = FileResultsRepository(args.results, fmt=args.format, sources=args.source or ())
    metadata = repository.read_metadata()
    current = build_metadata(config, engine.version())
    if repository.sources:
        current["sources"] = repository.source_entries()
    if metadata and repository.sources and metadata.get("sources") != current["sources"]:
        metadata = {**metadata, "sources": current["sources"]}
        repository.write_metadata(metadata)
    if metadata and metadata.get("config_hash") != current["config_hash"]:
        if not args.force:
            print(
                "ERREUR : la configuration a changé depuis le début de cette campagne (--force pour ignorer)"
            )
            return 4
        # Les tranches existantes restent valides (graines par cellule), l'historique des configurations est conservé.
        history = list(metadata.get("previous_config_hashes", []))
        history.append(str(metadata.get("config_hash")))
        current["previous_config_hashes"] = history
        current["created"] = metadata.get("created", current["created"])
        current["extended"] = build_metadata(config, engine.version())["created"]
        repository.write_metadata(current)
        metadata = current
    if not metadata:
        repository.write_metadata(current)
        metadata = current
    runner = CampaignRunner(
        config, engine, repository, metadata, progress=lambda m: print(m, flush=True)
    )
    try:
        summary = runner.run(
            time_limit=args.time_limit, force=args.force, selection=_selection(args)
        )
    except Cancelled:
        print("interrompu : relancer la même commande pour reprendre")
        return 130
    state = "terminée" if summary.complete else "à reprendre"
    print(
        f"campagne {state} : {summary.chunks_done} tranches jouées, {summary.chunks_skipped} déjà présentes, "
        f"{summary.games_played} parties en {summary.seconds:.0f} s"
    )
    return 0 if summary.complete else 5


def cmd_verify(args: argparse.Namespace) -> int:
    """Rejoue des tranches déjà enregistrées et vérifie qu'elles sont identiques
    (contrôle de déterminisme de la section 7)."""
    import random

    import pandas as pd

    from hexbandit.experiments.campaign import CampaignConfig, rows_from_records
    from hexbandit.experiments.repository import FileResultsRepository

    config = CampaignConfig.load(args.campaign)
    repository = FileResultsRepository(args.results, fmt=args.format, sources=args.source or ())
    stored = repository.load_all()
    chunks = [
        (cell, offset)
        for cell in config.cells()
        for offset in range(0, cell.games, config.campaign.chunk_size)
        if repository.has_chunk(cell.cell_id, offset)
    ]
    if not chunks:
        print("aucune tranche enregistrée")
        return 1
    rng = random.Random(args.seed)
    selection = rng.sample(chunks, min(args.chunks, len(chunks)))
    engine = _engine()
    keys = ["game_index", "seed", "first_color", "winner", "num_moves"]
    mismatches = 0
    for cell, offset in selection:
        n_games = min(config.campaign.chunk_size, cell.games - offset)
        records = engine.play_match(config.match_spec(cell, offset, n_games), args.threads)
        fresh = (
            pd.DataFrame(rows_from_records(config, cell, records, {}))[keys]
            .astype(str)
            .reset_index(drop=True)
        )
        old = stored[
            (stored["cell_id"] == cell.cell_id)
            & (stored["game_index"] >= offset)
            & (stored["game_index"] < offset + n_games)
        ]
        old = old.sort_values("game_index")[keys].astype(str).reset_index(drop=True)
        same = fresh.equals(old)
        mismatches += 0 if same else 1
        print(
            f"{cell.cell_id} parties {offset}-{offset + n_games - 1} : {'identiques' if same else 'DIFFÉRENTES'}"
        )
    print(f"{len(selection) - mismatches}/{len(selection)} tranches identiques")
    return 0 if mismatches == 0 else 6


def cmd_report(args: argparse.Namespace) -> int:
    """Analyse les résultats bruts et régénère graphiques et rapport."""
    from hexbandit.experiments.analysis import analyse
    from hexbandit.experiments.plots import plot_all
    from hexbandit.experiments.report import write_report
    from hexbandit.experiments.repository import FileResultsRepository

    repository = FileResultsRepository(args.results, fmt=args.format, sources=args.source or ())
    games = repository.load_all()
    if games.empty:
        print("aucun résultat")
        return 1
    analysis = analyse(games, n_boot=args.bootstrap, seed=args.seed)
    out = Path(args.out or Path(args.results) / "rapport")
    out.mkdir(parents=True, exist_ok=True)
    analysis.cells.to_csv(out / "cellules.csv", index=False)
    analysis.estimates_table.to_csv(out / "estimations.csv", index=False)
    analysis.global_rstar.to_csv(out / "rstar_modele_global.csv", index=False)
    figures = plot_all(analysis, out / "figures")
    tests = dict(analysis.tests)
    extended = None
    if not args.v1_only:
        from hexbandit.experiments.extended import extended_analysis
        from hexbandit.experiments.plots import plot_g6, plot_g7

        extended = extended_analysis(
            games, analysis, n_boot=max(50, args.bootstrap // 5), seed=args.seed
        )
        extended.save(out)
        figures["G6"] = plot_g6(extended.doubling, out / "figures" / "g6_elo_doublement.png")
        figures["G7"] = plot_g7(extended.comparison, out / "figures" / "g7_rstar_modeles.png")
        tests["v2"] = extended.tests()
    (out / "tests.json").write_text(json.dumps(tests, indent=1, default=float), encoding="utf-8")
    report = write_report(
        analysis, repository.read_metadata(), figures, out / "resultats.md", extended
    )
    print(f"rapport : {report}")
    for key, path in figures.items():
        print(f"{key} : {path}")
    return 0


def cmd_adaptive(args: argparse.Namespace) -> int:
    """Exécute (ou reprend) une étude adaptative de r*, puis écrit son rapport."""
    import pandas as pd

    from hexbandit.experiments.adaptive import AdaptiveConfig, AdaptiveRunner, results_frame
    from hexbandit.experiments.plots import plot_adaptive

    config = AdaptiveConfig.load(args.config)
    if args.threads:
        config = config.model_copy(
            update={"campaign": config.campaign.model_copy(update={"threads": args.threads})}
        )
    runner = AdaptiveRunner(
        config, _engine(args.fake), args.results, progress=lambda m: print(m, flush=True)
    )
    try:
        runner.run(time_limit=args.time_limit)
    except Cancelled:
        print("interrompu : relancer la même commande pour reprendre")
        return 130
    results = runner.results()
    summary = results_frame(config, results)
    if summary.empty:
        print("aucun lot joué")
        return 5
    plan = config.adaptive
    rows = []
    for r in results:
        post_steps = type(r)(r.algorithm, r.base_budget, r.size)
        for step in r.steps:
            post_steps.steps.append(step)
            _, median, lo, hi = post_steps.posterior(plan).alpha_summary()
            rows.append(
                {
                    "group": r.group_id,
                    "step": step.step,
                    "budget_first": step.budget_first,
                    "x": step.x,
                    "games": step.games,
                    "wins": step.wins,
                    "median": median,
                    "low": lo,
                    "high": hi,
                }
            )
    steps = pd.DataFrame(rows)
    out = Path(args.results) / "rapport"
    out.mkdir(parents=True, exist_ok=True)
    summary.to_csv(out / "rstar_adaptatif.csv", index=False)
    steps.to_csv(out / "lots.csv", index=False)
    figure = plot_adaptive(summary, steps, out / "figures" / "plan_adaptatif.png")
    comparison = None
    if args.compare:
        comparison = _compare_with_grid(summary, args.compare, args.source or ())
        comparison.to_csv(out / "comparaison_grille.csv", index=False)
    _write_adaptive_report(config, summary, comparison, figure, out / "resultats.md")
    print(f"rapport : {out / 'resultats.md'}")
    complete = bool(summary["complete"].all()) and len(summary) == len(config.groups())
    return 0 if complete else 5


def _compare_with_grid(summary: Any, directory: str, sources: Sequence[str]) -> Any:
    """Précision et coût du plan adaptatif comparés à la grille fixe."""
    from hexbandit.experiments.analysis import cell_table, estimates_frame, stratified_estimates
    from hexbandit.experiments.repository import FileResultsRepository

    games = FileResultsRepository(directory, sources=sources).load_all()
    estimates = estimates_frame(stratified_estimates(cell_table(games), n_boot=400))
    keys = ["algorithm", "base_budget", "size"]
    merged = summary.merge(
        estimates[[*keys, "games", "log2_rstar", "log2_rstar_low", "log2_rstar_high"]],
        on=keys,
        suffixes=("", "_grid"),
    )
    merged["width_grid"] = merged["log2_rstar_high_grid"] - merged["log2_rstar_low_grid"]
    merged["games_ratio"] = merged["games"] / merged["games_grid"]
    merged["width_ratio"] = merged["width"] / merged["width_grid"]
    return merged


def _write_adaptive_report(
    config: Any, summary: Any, comparison: Any, figure: Path, path: Path
) -> None:
    def ratio(v: float) -> str:
        return "-" if v != v else f"{2.0**v:.2f}"

    plan = config.adaptive
    lines = [
        f"# Plan adaptatif de r* - « {config.campaign.name} »",
        "",
        f"Lots de {plan.batch} parties au rapport le plus informatif (méthode Psi), arrêt quand "
        f"l'intervalle de crédibilité à 95 % de log2 r* est plus étroit que {plan.target_width} "
        f"(au plus {plan.max_games} parties par groupe). Modèle local : logit P = β·(log2 r - log2 r*).",
        "",
        "| algorithme | B0 | N | parties | lots | r* [IC 95 %] | largeur (log2) | terminé |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for r in summary.itertuples(index=False):
        lines.append(
            f"| {r.algorithm} | {r.base_budget} | {r.size} | {r.games} | {r.steps} | "
            f"{ratio(r.log2_rstar)} [{ratio(r.log2_rstar_low)}, {ratio(r.log2_rstar_high)}] | "
            f"{r.width:.2f} | {'oui' if r.complete else 'non'} |"
        )
    if comparison is not None and len(comparison):
        lines += [
            "",
            "## Comparaison avec la grille fixe",
            "",
            "| algorithme | B0 | N | r* adaptatif | r* grille (2 pentes) | parties (adaptatif / grille) | largeur (adaptatif / grille) |",
            "|---|---|---|---|---|---|---|",
        ]
        for r in comparison.itertuples(index=False):
            lines.append(
                f"| {r.algorithm} | {r.base_budget} | {r.size} | {ratio(r.log2_rstar)} "
                f"[{ratio(r.log2_rstar_low)}, {ratio(r.log2_rstar_high)}] | {ratio(r.log2_rstar_grid)} "
                f"[{ratio(r.log2_rstar_low_grid)}, {ratio(r.log2_rstar_high_grid)}] | "
                f"{r.games} / {r.games_grid} ({r.games_ratio:.0%}) | {r.width:.2f} / {r.width_grid:.2f} |"
            )
    lines += ["", f"![plan adaptatif](figures/{figure.name})", ""]
    path.write_text("\n".join(lines), encoding="utf-8")


def cmd_tournament(args: argparse.Namespace) -> int:
    """Tournoi toutes rondes ; classement Elo (Bradley-Terry avec avantage du trait)."""
    from hexbandit.experiments.tournament import TournamentConfig, run_tournament

    config = TournamentConfig.load(args.config)
    if args.threads:
        config = config.model_copy(
            update={"tournament": config.tournament.model_copy(update={"threads": args.threads})}
        )
    result = run_tournament(config, _engine(args.fake), progress=lambda m: print(m, flush=True))
    standings = result.standings()
    reference = standings.loc[standings["elo_se"] == 0, "player"]
    print(
        f"\nclassement ({len(result.games)} parties, avantage du trait {result.fit.advantage_elo:+.0f} Elo ;"
        f" ± : intervalle à 95 % de l'écart à {reference.iloc[0] if len(reference) else '?'}) :"
    )
    for r in standings.itertuples(index=False):
        print(f"  {r.player:<20} {r.elo:+7.0f} ± {1.96 * r.elo_se:4.0f} Elo   {r.wins}/{r.games}")
    if args.out:
        out = Path(args.out)
        out.mkdir(parents=True, exist_ok=True)
        result.games.to_csv(out / "parties.csv", index=False)
        standings.to_csv(out / "classement.csv", index=False)
        print(f"résultats : {out}")
    return 0


def _common_game(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--size", type=int, default=14, help="taille du plateau (2 à 19)")
    parser.add_argument(
        "--kind",
        choices=["hex", "reverse_hex", "quarto", "quarto_squares", "chameleon"],
        default="hex",
    )
    parser.add_argument("--swap", action="store_true", help="règle du gâteau")
    parser.add_argument("--seed", type=int, default=0)


def _source_option(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--source",
        action="append",
        metavar="DIR",
        help="répertoire de résultats lu en plus (tranches déjà jouées, répétable)",
    )


def _selection_options(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--cells",
        action="append",
        metavar="MOTIF",
        help="cellules à traiter (motif fnmatch sur l'identifiant, répétable), ex. '*_B2048_N14_*'",
    )
    parser.add_argument(
        "--games-limit",
        type=int,
        metavar="N",
        help="seulement les N premières parties de chaque cellule (tranches entières)",
    )
    parser.add_argument(
        "--shard", metavar="i/n", help="une tranche sur n à partir de la i-ème (répartition)"
    )


def describe_versions() -> str:
    """Versions du paquet, de l'interpréteur et du moteur natif (diagnostic)."""
    lines = [f"hexbandit {__version__}", python_label()]
    try:
        native = _native.version()
    except (HexbanditError, OSError) as exc:
        lines.append(f"moteur natif : absent ({exc})")
    else:
        detail = ", ".join(f"{key}={value}" for key, value in sorted(native.items()))
        lines.append(f"moteur natif : {_native.library_path()} ({detail})")
    return "\n".join(lines)


class _VersionAction(argparse.Action):
    """``--version`` : affiche :func:`describe_versions` puis quitte."""

    def __init__(
        self,
        option_strings: Sequence[str],
        dest: str = argparse.SUPPRESS,
        default: str = argparse.SUPPRESS,
        help: str | None = None,
    ) -> None:
        super().__init__(
            option_strings=option_strings, dest=dest, default=default, nargs=0, help=help
        )

    def __call__(
        self,
        parser: argparse.ArgumentParser,
        namespace: argparse.Namespace,
        values: str | Sequence[Any] | None,
        option_string: str | None = None,
    ) -> None:
        print(describe_versions())
        parser.exit()


def build_parser() -> argparse.ArgumentParser:
    """Analyseur de la ligne de commande."""
    parser = argparse.ArgumentParser(prog="hexbandit", description="Hex, MCTS et MC-RAVE")
    parser.add_argument(
        "--version", action=_VersionAction, help="versions et diagnostic, puis quitte"
    )
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("play", help="jouer une partie IA contre IA")
    _common_game(p)
    p.add_argument("--blue", type=parse_player, default=parse_player("rave:2000"))
    p.add_argument("--red", type=parse_player, default=parse_player("uct:2000"))
    first: list[Color] = ["blue", "red"]
    p.add_argument("--first", choices=first, default="blue")
    p.add_argument("--report", choices=["none", "summary", "full"], default="full")
    p.add_argument("--opening", default="", help="coups imposés, séparés par des virgules")
    p.add_argument("--out", help="enregistrement JSON")
    p.add_argument("--image", help="image PNG de la position finale")
    p.add_argument("--gif", help="animation GIF ou MP4")
    p.set_defaults(func=cmd_play)

    p = sub.add_parser("replay", help="rejouer un enregistrement")
    p.add_argument("record")
    p.add_argument("--out", help="image finale")
    p.add_argument("--frames", help="répertoire des images coup par coup")
    p.add_argument("--gif")
    p.add_argument("--mp4")
    p.add_argument("--fps", type=float, default=4.0)
    p.add_argument("--heatmap", type=int, help="numéro du coup pour la carte de chaleur")
    p.add_argument("--heatmap-out")
    p.add_argument("--metric", choices=["visits", "value", "amaf"], default="visits")
    p.set_defaults(func=cmd_replay)

    p = sub.add_parser("view", help="visionneuse web autonome d'enregistrements")
    p.add_argument("records", nargs="+", help="enregistrements JSON (une partie ou une liste)")
    p.add_argument("--out", help="page HTML produite (défaut : à côté du premier enregistrement)")
    p.set_defaults(func=cmd_view)

    p = sub.add_parser("search", help="analyser une position")
    _common_game(p)
    p.add_argument("--first", choices=["blue", "red"], default="blue")
    p.add_argument("--moves", default="", help="coups depuis le début, séparés par des virgules")
    p.add_argument("--position", help="position textuelle (lignes séparées par /, puis b ou r)")
    p.add_argument("--player", type=parse_player, default=parse_player("rave:10000"))
    p.add_argument("--top", type=int, default=10)
    p.set_defaults(func=cmd_search)

    p = sub.add_parser("match", help="match A contre B")
    _common_game(p)
    p.add_argument("--a", type=parse_player, required=True)
    p.add_argument("--b", type=parse_player, required=True)
    p.add_argument("--games", type=int, default=100)
    p.add_argument("--threads", type=int, default=2)
    p.add_argument(
        "--seating",
        choices=["alternate_first", "a_first_alternate_colors"],
        default="alternate_first",
    )
    p.add_argument("--out", help="fichier JSON auquel ajouter le bilan")
    p.set_defaults(func=cmd_match)

    p = sub.add_parser("estimate", help="estimer le coût d'une campagne")
    p.add_argument("campaign")
    p.set_defaults(func=cmd_estimate)

    p = sub.add_parser("run-campaign", help="exécuter ou reprendre une campagne")
    p.add_argument("campaign")
    p.add_argument("--results", required=True, help="répertoire des résultats")
    p.add_argument("--threads", type=int)
    p.add_argument("--time-limit", type=float, help="arrêt propre après ce nombre de secondes")
    p.add_argument("--force", action="store_true", help="ignorer le plafond de coût")
    p.add_argument("--format", choices=["csv", "parquet"], default="csv")
    p.add_argument("--fake", action="store_true", help="moteur factice (tests)")
    _selection_options(p)
    _source_option(p)
    p.set_defaults(func=cmd_run_campaign)

    p = sub.add_parser(
        "verify", help="rejouer des tranches enregistrées et vérifier le déterminisme"
    )
    p.add_argument("campaign")
    p.add_argument("--results", required=True)
    p.add_argument("--chunks", type=int, default=5)
    p.add_argument(
        "--threads",
        type=int,
        default=1,
        help="nombre de threads (différent de la campagne)",
    )
    p.add_argument("--seed", type=int, default=0)
    p.add_argument("--format", choices=["csv", "parquet"], default="csv")
    _source_option(p)
    p.set_defaults(func=cmd_verify)

    p = sub.add_parser("report", help="analyser et produire graphiques et rapport")
    p.add_argument("results")
    p.add_argument("--out")
    p.add_argument("--bootstrap", type=int, default=1000)
    p.add_argument("--seed", type=int, default=20261115)
    p.add_argument("--format", choices=["csv", "parquet"], default="csv")
    p.add_argument(
        "--v1-only",
        action="store_true",
        help="sans les analyses Elo, modèle mixte, splines, puissance",
    )
    _source_option(p)
    p.set_defaults(func=cmd_report)

    p = sub.add_parser("adaptive", help="estimer r* par un plan adaptatif")
    p.add_argument("config", help="fichier TOML de l'étude adaptative")
    p.add_argument("--results", required=True, help="répertoire des résultats")
    p.add_argument("--threads", type=int)
    p.add_argument("--time-limit", type=float, help="arrêt propre après ce nombre de secondes")
    p.add_argument("--fake", action="store_true", help="moteur factice (tests)")
    p.add_argument(
        "--compare",
        metavar="DIR",
        help="résultats d'une campagne à grille fixe, pour comparer précision et coût",
    )
    _source_option(p)
    p.set_defaults(func=cmd_adaptive)

    p = sub.add_parser("tournament", help="tournoi toutes rondes et classement Elo")
    p.add_argument("config", help="fichier TOML : [tournament] et [players.<nom>]")
    p.add_argument("--threads", type=int)
    p.add_argument("--fake", action="store_true", help="moteur factice (tests)")
    p.add_argument("--out", help="répertoire où écrire parties et classement (CSV)")
    p.set_defaults(func=cmd_tournament)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    """Point d'entrée."""
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        code: int = args.func(args)
    except KeyboardInterrupt:
        print("interrompu", file=sys.stderr)
        return 130
    except HexbanditError as exc:
        print(f"erreur : {exc}", file=sys.stderr)
        return 2
    return code


if __name__ == "__main__":
    sys.exit(main())
