"""Rapport de résultats en Markdown, régénéré depuis les données brutes.

Le rapport reprend les métadonnées de la campagne, la table des estimations
par groupe, les verdicts des hypothèses H1 à H4, le contrôle de couleur, les
coefficients du modèle global, les graphiques G1 à G5 et, en annexe, la table
complète des cellules (vue tabulaire des graphiques).
"""

from __future__ import annotations

import math
import os
from pathlib import Path
from typing import Any

import pandas as pd

from hexbandit.experiments.analysis import Analysis
from hexbandit.experiments.extended import ExtendedAnalysis, extended_markdown

ALGO = {"uct": "UCT", "rave": "MC-RAVE"}


def _ratio(log2: float) -> str:
    if not math.isfinite(log2):
        return "-"
    r = 2.0**log2
    return f"{r:.2f}" if r >= 1 else f"1/{1 / r:.2f}"


def _p(value: float) -> str:
    if not math.isfinite(value):
        return "-"
    return "< 1e-4" if value < 1e-4 else f"{value:.4f}"


def _table(frame: pd.DataFrame) -> str:
    header = "| " + " | ".join(str(c) for c in frame.columns) + " |"
    sep = "|" + "|".join("---" for _ in frame.columns) + "|"
    rows = ["| " + " | ".join(str(v) for v in row) + " |" for row in frame.itertuples(index=False)]
    return "\n".join([header, sep, *rows])


def estimates_markdown(analysis: Analysis) -> str:
    """Table des estimations par (algorithme, B0, N)."""
    rows = []
    for e in analysis.estimates:
        lo, hi = e.log2_rstar_ci
        llo, lhi = e.log2_rstar_linear_ci
        rows.append(
            {
                "algorithme": ALGO.get(e.algorithm, e.algorithm),
                "B0": e.base_budget,
                "N": e.size,
                "parties": e.games,
                "P_premier (r = 1)": f"{e.p_first_r1:.3f} [{e.p_first_r1_ci[0]:.3f}, {e.p_first_r1_ci[1]:.3f}]",
                "b₋ (r < 1)": f"{e.slope_minus:.2f}",
                "b₊ (r > 1)": f"{e.slope_plus:.2f}",
                "asymétrie p": _p(e.asymmetry_p_value),
                "r* (2 pentes) [IC 95 %]": f"{_ratio(e.log2_rstar)} [{_ratio(lo)}, {_ratio(hi)}]"
                + ("" if e.in_range else " (extrapolé)"),
                "r* (linéaire) [IC 95 %]": f"{_ratio(e.log2_rstar_linear)} [{_ratio(llo)}, {_ratio(lhi)}]",
                "adéquation p (lin. / 2 pentes)": f"{_p(e.lack_of_fit_p_linear)} / {_p(e.lack_of_fit_p_two_slopes)}",
                "Bleu à r = 1": f"{e.blue_rate_r1:.3f} (p = {_p(e.blue_p_value_r1)})",
            }
        )
    return _table(pd.DataFrame(rows))


def hypotheses_markdown(analysis: Analysis) -> str:
    """Verdicts des hypothèses H1 à H4 et du contrôle de couleur."""
    t = analysis.tests
    lines = []
    h1 = t["H1"]
    lines.append(
        f"* **H1 (P_premier croît avec r)** : pente du modèle linéaire significativement positive "
        f"(p < 0,05) dans {h1['groups_positive']} groupes sur {h1['groups']} ; les deux pentes du modèle "
        f"à deux pentes sont positives dans {h1['groups_both_slopes_positive']} groupes. Asymétrie "
        f"significative (b₋ > b₊, p < 0,05) dans {h1['groups_asymmetric']} groupes : perdre du budget "
        f"coûte plus que n'en rapporte le même gain."
    )
    h2 = t["H2"]
    lines.append(
        f"* **H2 (r* < 1)** : avec le modèle à deux pentes, intervalle de r* entièrement sous 1 dans "
        f"{h2['groups_rstar_below_1']} groupes sur {h2['groups']}, entièrement au-dessus de 1 dans "
        f"{h2['groups_rstar_above_1']} (modèle linéaire : {h2['groups_rstar_below_1_linear']} et "
        f"{h2['groups_rstar_above_1_linear']})."
    )
    if "H3" in t:
        h3 = t["H3"]
        lines.append(
            f"* **H3 (r* dépend de N)** : rapport de vraisemblance des termes en N du modèle global à deux "
            f"pentes, χ² = {h3['lrt_stat']:.1f} à {h3['df']} ddl, p = {_p(h3['p_value'])} (modèle linéaire : "
            f"χ² = {h3['lrt_stat_linear']:.1f} à {h3['df_linear']} ddl, p = {_p(h3['p_value_linear'])})."
        )
    if "H4" in t:
        h4 = t["H4"]
        coefs = ", ".join(f"{ALGO.get(a, a)} : {v:+.3f}" for a, v in h4["alg_coef"].items())
        lines.append(
            f"* **H4 (RAVE modifie r*)** : effet additif de l'algorithme sur le logit ({coefs}), "
            f"χ² = {h4['lrt_stat']:.1f} à {h4['df']} ddl, p = {_p(h4['p_value'])} ; interaction "
            f"algorithme × pentes : χ² = {h4['slope_interaction_stat']:.1f} à {h4['slope_interaction_df']} ddl, "
            f"p = {_p(h4['slope_interaction_p_value'])}."
        )
        if h4["differences"]:
            diffs = pd.DataFrame(
                [
                    {
                        "B0": d["base_budget"],
                        "N": d["size"],
                        "Δ log2 r* (RAVE - UCT)": f"{d['difference']:+.2f}",
                        "IC 95 %": f"[{d['low']:+.2f}, {d['high']:+.2f}]",
                    }
                    for d in h4["differences"]
                ]
            )
            lines.append("\n" + _table(diffs))
    if "color_bias" in t:
        c = t["color_bias"]
        lines.append(
            f"* **Contrôle de couleur** (r = 1, toutes tailles) : Bleu gagne {c['blue_wins']} parties sur "
            f"{c['games']} ({c['blue_rate']:.3f}), test binomial contre 0,5 : p = {_p(c['p_value'])}."
        )
    return "\n".join(lines)


def global_model_markdown(analysis: Analysis) -> str:
    """Coefficients des modèles globaux et r* qu'ils prédisent."""
    parts = []
    for title, fit, key in (
        (
            "Modèle linéaire du cahier des charges, logit(P) ~ log2(r) × N + algorithme + B0",
            analysis.global_fit,
            "global_fit",
        ),
        (
            "Modèle à deux pentes, logit(P) ~ (min(log2 r, 0) + max(log2 r, 0)) × N + algorithme + B0",
            analysis.global_fit_two_slopes,
            "global_fit_two_slopes",
        ),
    ):
        gf = analysis.tests[key]
        coefs = pd.DataFrame(
            [
                {
                    "terme": n,
                    "coefficient": f"{b:+.4f}",
                    "écart-type": f"{s:.4f}",
                    "z": f"{z:+.2f}",
                    "p": _p(p),
                }
                for n, b, s, z, p in fit.summary_rows()
            ]
        )
        parts.append(
            f"**{title}** - déviance {gf['deviance']:.1f} pour {gf['df_resid']} degrés de liberté "
            f"(test d'adéquation : p = {_p(gf['lack_of_fit_p'])}).\n\n<details><summary>Coefficients</summary>\n\n"
            + _table(coefs)
            + "\n\n</details>"
        )
    g = analysis.global_rstar
    rstar = pd.DataFrame(
        {
            "algorithme": g["algorithm"].map(lambda a: ALGO.get(a, a)),
            "B0": g["base_budget"],
            "N": g["size"],
            "r* (2 pentes) [IC 95 %]": [
                f"{_ratio(a)} [{_ratio(b)}, {_ratio(c)}]"
                for a, b, c in zip(
                    g["log2_rstar_two_slopes"],
                    g["log2_rstar_two_slopes_low"],
                    g["log2_rstar_two_slopes_high"],
                    strict=True,
                )
            ],
            "r* (linéaire) [IC 95 %]": [
                f"{_ratio(a)} [{_ratio(b)}, {_ratio(c)}]"
                for a, b, c in zip(
                    g["log2_rstar_linear"],
                    g["log2_rstar_linear_low"],
                    g["log2_rstar_linear_high"],
                    strict=True,
                )
            ],
        }
    )
    parts.append(
        "Ratio d'équilibre prédit par les modèles globaux (bootstrap des cellules) :\n\n"
        + _table(rstar)
    )
    return "\n\n".join(parts)


def cells_markdown(analysis: Analysis) -> str:
    """Table complète des cellules (vue tabulaire des graphiques)."""
    c = analysis.cells
    frame = pd.DataFrame(
        {
            "algorithme": c["algorithm"].map(lambda a: ALGO.get(a, a)),
            "B0": c["base_budget"],
            "N": c["size"],
            "r": c["ratio_exponent"].map(lambda k: _ratio(float(k))),
            "parties": c["n"],
            "victoires du premier": c["wins"],
            "P_premier": c["p_first"].map(lambda v: f"{v:.3f}"),
            "IC de Wilson 95 %": [
                f"[{a:.3f}, {b:.3f}]" for a, b in zip(c["ci_low"], c["ci_high"], strict=True)
            ],
            "p (≠ 0,5)": c["p_value"].map(_p),
            "coups moyens": c["mean_moves"].map(lambda v: f"{v:.1f}"),
        }
    )
    return _table(frame)


def write_report(
    analysis: Analysis,
    metadata: dict[str, Any],
    figures: dict[str, Path],
    path: Path,
    extended: ExtendedAnalysis | None = None,
) -> Path:
    """Écrit le rapport Markdown ; les figures sont référencées en chemins
    relatifs. ``extended`` ajoute d'autres analyses (Elo, modèle mixte,
    splines monotones, puissance)."""
    path.parent.mkdir(parents=True, exist_ok=True)

    def rel(p: Path) -> str:
        return os.path.relpath(p, path.parent).replace(os.sep, "/")

    versions = metadata.get("versions", {})
    engine = metadata.get("engine", {})
    cfg = metadata.get("config", {})
    plan = cfg.get("plan", {})
    total = int(analysis.cells["n"].sum())
    parts = [
        f"# Résultats de la campagne « {metadata.get('campaign', '?')} »",
        "",
        "Rapport généré automatiquement par `hexbandit report` à partir des données brutes.",
        "",
        "## Contexte",
        "",
        f"* Parties analysées : {total} réparties en {len(analysis.cells)} cellules.",
        f"* Tailles : {plan.get('sizes', '?')} ; exposants k de r = 2^k : {plan.get('ratio_exponents', '?')} ; "
        f"budgets de base B0 : {plan.get('base_budgets', '?')} ; algorithmes : {plan.get('algorithms', '?')}.",
        f"* Moteur {engine.get('engine_version', '?')} ; hexbandit {metadata.get('hexbandit_version', '?')} ; "
        f"Git {metadata.get('git_hash', '?')} ; configuration {metadata.get('config_hash', '?')} ; "
        f"créée le {metadata.get('created', '?')}.",
        f"* Python {versions.get('python', '?')}, NumPy {versions.get('numpy', '?')}, pandas {versions.get('pandas', '?')}, "
        f"SciPy {versions.get('scipy', '?')}, Matplotlib {versions.get('matplotlib', '?')}.",
        "",
        "## Estimations par groupe",
        "",
        "Pour chaque (algorithme, B0, N), deux modèles ajustés avec correction de Firth : linéaire "
        "(logit P = a + b·x, x = log2 r, cahier des charges) et à deux pentes (logit P = a + b₋·min(x, 0) "
        "+ b₊·max(x, 0)). r* est la valeur de r où P = 0,5 ; intervalles par bootstrap paramétrique des "
        "cellules. La colonne « adéquation » donne la p-valeur du test de déviance (une petite valeur "
        "signale un modèle mal ajusté).",
        "",
        estimates_markdown(analysis),
        "",
        "## Hypothèses",
        "",
        hypotheses_markdown(analysis),
        "",
        "## Modèles globaux",
        "",
        "N est traité comme un facteur ; les tests H3 et H4 utilisent le modèle à deux pentes.",
        "",
        global_model_markdown(analysis),
        "",
        "## Graphiques",
        "",
    ]
    captions = {
        "G1": "P_premier en fonction du rapport de budget, une courbe par taille",
        "G2": "Carte de chaleur N × r",
        "G3": "Ratio d'équilibre r* en fonction de N",
        "G4": "Avantage du premier joueur à budgets égaux",
        "G5": "Contrôle du biais de couleur",
    }
    for key, figure in figures.items():
        if key in captions:
            parts += [f"### {key} · {captions[key]}", "", f"![{key}]({rel(figure)})", ""]
    if extended is not None:
        parts += [extended_markdown(extended, figures, rel), ""]
    parts += ["## Annexe : table des cellules", "", cells_markdown(analysis), ""]
    path.write_text("\n".join(parts), encoding="utf-8")
    return path
