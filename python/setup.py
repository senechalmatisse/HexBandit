"""Construction de la roue de hexbandit.

La configuration est dans ``pyproject.toml`` ; ce fichier ne sert qu'à
étiqueter la roue comme propre à la plateforme : elle contient la
bibliothèque native (``hexbandit/_lib``), chargée par ctypes et non comme une
extension CPython. Une seule roue par plateforme convient donc à toutes les
versions de Python prises en charge (étiquette ``py3-none-<plateforme>``).
"""

from __future__ import annotations

from setuptools import setup
from setuptools.command.bdist_wheel import bdist_wheel
from setuptools.dist import Distribution


class NativeDistribution(Distribution):
    """Distribution à contenu natif : fichiers à la racine de la roue (platlib)."""

    def has_ext_modules(self) -> bool:
        return True


class PlatformWheel(bdist_wheel):
    """Roue ``py3-none-<plateforme>`` : aucune dépendance à l'ABI de CPython."""

    def get_tag(self) -> tuple[str, str, str]:
        _, _, platform = super().get_tag()
        return "py3", "none", platform


setup(distclass=NativeDistribution, cmdclass={"bdist_wheel": PlatformWheel})
