"""Stockage des résultats.

Arborescence d'un répertoire de résultats ::

    metadata.json                       configuration, versions, hash Git, date
    chunks/<cellule>/chunk_000000.csv   une ligne par partie

Chaque tranche est écrite dans un fichier temporaire puis renommée
(opération atomique) : une interruption ne laisse jamais de fichier partiel.
Le format Parquet est utilisé si ``pyarrow`` est installé et demandé.

Un répertoire peut déclarer des **sources** en lecture seule (champ
``sources`` de ``metadata.json``, chemins relatifs au répertoire) : leurs
tranches comptent comme déjà jouées et sont lues avec les siennes. La
campagne complète réutilise ainsi les tranches de la campagne réduite, qui
contiennent exactement les mêmes parties (même graine maître, mêmes
identifiants de cellules, même taille de tranche), sans les recopier ; des
machines qui se partagent une campagne (``--shard``) fusionnent leurs
résultats de la même façon.
"""

from __future__ import annotations

import importlib.util
import json
import os
import platform
import subprocess
from collections.abc import Sequence
from datetime import UTC, datetime
from pathlib import Path
from typing import TYPE_CHECKING, Any, Literal

import pandas as pd

import hexbandit

if TYPE_CHECKING:
    from hexbandit.experiments.campaign import CampaignConfig, ChunkResult

Format = Literal["csv", "parquet"]


class FileResultsRepository:
    """Résultats en fichiers CSV (ou Parquet) par tranche."""

    def __init__(
        self, root: str | Path, fmt: Format = "csv", sources: Sequence[str | Path] = ()
    ) -> None:
        if fmt == "parquet" and importlib.util.find_spec("pyarrow") is None:
            raise RuntimeError("format parquet demandé mais pyarrow n'est pas installé")
        self.root = Path(root)
        self.fmt: Format = fmt
        self.chunks = self.root / "chunks"
        self.chunks.mkdir(parents=True, exist_ok=True)
        # Sources déclarées : relatives au répertoire ; sources passées en
        # argument : relatives au répertoire courant.
        declared = [self.root / str(s) for s in self.read_metadata().get("sources", [])]
        self.sources: list[Path] = []
        for source in [*declared, *(Path(s) for s in sources)]:
            path = source.resolve()
            if path == self.root.resolve() or path in self.sources:
                continue
            if not (path / "chunks").is_dir():
                raise RuntimeError(f"source de résultats introuvable : {path}")
            self.sources.append(path)

    def source_entries(self) -> list[str]:
        """Sources à enregistrer dans ``metadata.json`` (chemins relatifs si possible)."""
        out = []
        for path in self.sources:
            try:
                out.append(os.path.relpath(path, self.root.resolve()))
            except ValueError:  # autre lecteur (Windows)
                out.append(str(path))
        return out

    def _path(self, cell_id: str, offset: int) -> Path:
        return self.chunks / cell_id / f"chunk_{offset:06d}.{self.fmt}"

    def _source_path(self, source: Path, cell_id: str, offset: int) -> Path:
        return source / "chunks" / cell_id / f"chunk_{offset:06d}.{self.fmt}"

    def has_chunk(self, cell_id: str, offset: int) -> bool:
        """Vrai si la tranche existe déjà (ici ou dans une source)."""
        if self._path(cell_id, offset).exists():
            return True
        return any(self._source_path(s, cell_id, offset).exists() for s in self.sources)

    def save(self, chunk: ChunkResult) -> None:
        """Écriture atomique d'une tranche."""
        path = self._path(chunk.cell.cell_id, chunk.offset)
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(path.suffix + ".tmp")
        frame = pd.DataFrame(chunk.rows)
        if self.fmt == "csv":
            frame.to_csv(tmp, index=False)
        else:
            frame.to_parquet(tmp, index=False)
        os.replace(tmp, path)

    def chunk_files(self) -> list[Path]:
        """Fichiers de tranches, sources comprises (une tranche présente ici
        masque celle d'une source)."""
        found: dict[tuple[str, str], Path] = {}
        for base in [self.chunks, *(s / "chunks" for s in self.sources)]:
            for path in sorted(base.glob(f"*/chunk_*.{self.fmt}")):
                found.setdefault((path.parent.name, path.name), path)
        return [found[key] for key in sorted(found)]

    def load_all(self) -> pd.DataFrame:
        """Toutes les parties (sources comprises), triées par cellule et indice."""
        files = self.chunk_files()
        if not files:
            return pd.DataFrame()
        reader = pd.read_csv if self.fmt == "csv" else pd.read_parquet
        frames = [reader(f) for f in files]
        data = pd.concat(frames, ignore_index=True)
        return data.sort_values(["cell_id", "game_index"], kind="stable").reset_index(drop=True)

    def write_metadata(self, metadata: dict[str, Any]) -> Path:
        """Écrit ``metadata.json``."""
        path = self.root / "metadata.json"
        path.write_text(json.dumps(metadata, indent=2, ensure_ascii=False), encoding="utf-8")
        return path

    def read_metadata(self) -> dict[str, Any]:
        """Lit ``metadata.json`` (vide s'il n'existe pas)."""
        path = self.root / "metadata.json"
        if not path.exists():
            return {}
        data: dict[str, Any] = json.loads(path.read_text(encoding="utf-8"))
        return data


def git_hash(start: Path | None = None) -> str:
    """Hash Git du dépôt (``unknown`` hors dépôt)."""
    try:
        out = subprocess.run(
            ["git", "rev-parse", "--short=12", "HEAD"],
            cwd=start or Path(__file__).resolve().parent,
            capture_output=True,
            text=True,
            check=True,
            timeout=10,
        )
        dirty = subprocess.run(
            ["git", "status", "--porcelain", "--untracked-files=no"],
            cwd=start or Path(__file__).resolve().parent,
            capture_output=True,
            text=True,
            check=True,
            timeout=10,
        )
        return out.stdout.strip() + ("-dirty" if dirty.stdout.strip() else "")
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def library_versions() -> dict[str, str]:
    """Versions des bibliothèques de la pile Python."""
    versions = {"python": platform.python_version(), "hexbandit": hexbandit.__version__}
    for name in ("numpy", "pandas", "scipy", "matplotlib", "pydantic", "pyarrow", "statsmodels"):
        try:
            module = __import__(name)
            versions[name] = str(getattr(module, "__version__", "?"))
        except ImportError:
            versions[name] = "absent"
    return versions


def build_metadata(config: CampaignConfig, engine_version: dict[str, Any]) -> dict[str, Any]:
    """Métadonnées d'une campagne."""
    return {
        "campaign": config.campaign.name,
        "config": config.model_dump(mode="json"),
        "config_hash": config.config_hash(),
        "engine": engine_version,
        "versions": library_versions(),
        "hexbandit_version": hexbandit.__version__,
        "git_hash": git_hash(),
        "created": datetime.now(UTC).isoformat(timespec="seconds"),
        "host": {"platform": platform.platform(), "cpus": os.cpu_count()},
    }
