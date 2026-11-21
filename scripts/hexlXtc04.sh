#!/bin/bash
#
# FONCTION : Construction de la roue Python de hexbandit pour la plateforme
#            courante (bibliotheque native incluse, etiquette
#            py3-none-<plateforme>). Sous Linux, l'etiquette manylinux est
#            deduite des versions de symboles glibc requises par la
#            bibliotheque. Sans acces reseau : setuptools et wheel doivent etre installes.
#
# PARAMETRES : Ce programme prend en parametre
# (facultatif) 1: le repertoire de sortie des roues (dist par defaut)
# (facultatif) 2: la toolchain Rust a utiliser (sinon RUSTUP_TOOLCHAIN,
#                 sinon celle de rust-toolchain.toml)
#

V_Vc_Nom_Script="hexlXtc04"
V_Vc_Rep_Script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
V_Vc_Rep_Racine="$(cd "${V_Vc_Rep_Script}/.." && pwd)"
V_Vc_Rep_Sortie="${1:-dist}"
V_Vc_Toolchain="${2:-${RUSTUP_TOOLCHAIN}}"
V_Vc_Roue=""
V_Vc_Glibc=""
V_Vc_Etiquette=""
V_N_Code_Retour=0

. "${V_Vc_Rep_Script}/hexlXtc00.sh"

# Bibliotheque native
bash "${V_Vc_Rep_Script}/hexlXtc01.sh" release "${V_Vc_Toolchain}"
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  f_Log "ERREUR : bibliotheque native non construite (code ${V_N_Code_Retour})"
  exit "${V_N_Code_Retour}"
fi

f_Python_Choisir
if ! "${V_Vc_Python}" -m pip --version > /dev/null 2>&1; then
  f_Log "ERREUR : pip absent de l'interpreteur Python"
  exit 3
fi
f_Python_Controler setuptools wheel
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  exit 3
fi

# Roue
mkdir -p "${V_Vc_Rep_Sortie}"
V_Vc_Rep_Sortie="$(cd "${V_Vc_Rep_Sortie}" && pwd)"
rm -rf "${V_Vc_Rep_Racine}/python/build"
f_Log "Construction de la roue dans ${V_Vc_Rep_Sortie}"
"${V_Vc_Python}" -m pip wheel --no-deps --no-build-isolation -w "${V_Vc_Rep_Sortie}" "${V_Vc_Rep_Racine}/python"
V_N_Code_Retour=$?
rm -rf "${V_Vc_Rep_Racine}/python/build"
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  f_Log "ERREUR : echec de la construction de la roue (code ${V_N_Code_Retour})"
  exit "${V_N_Code_Retour}"
fi
V_Vc_Roue="$(ls -t "${V_Vc_Rep_Sortie}"/hexbandit-*.whl 2> /dev/null | head -n 1)"
if [ -z "${V_Vc_Roue}" ]; then
  f_Log "ERREUR : roue introuvable dans ${V_Vc_Rep_Sortie}"
  exit 4
fi

# Etiquette manylinux (Linux) : plus haute version GLIBC_x.y requise
if [ "$(uname -s)" = "Linux" ] && command -v objdump > /dev/null 2>&1; then
  V_Vc_Glibc="$(objdump -T "${V_Vc_Rep_Racine}/python/hexbandit/_lib/libhexbandit_native.so" | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sed 's/GLIBC_//' | sort -V | tail -n 1)"
  if [ -n "${V_Vc_Glibc}" ]; then
    V_Vc_Etiquette="manylinux_${V_Vc_Glibc/./_}_$(uname -m)"
    f_Log "Etiquette de plateforme : ${V_Vc_Etiquette} (glibc ${V_Vc_Glibc} requise)"
    "${V_Vc_Python}" -m wheel tags --remove --platform-tag "${V_Vc_Etiquette}" "${V_Vc_Roue}" > /dev/null
    V_N_Code_Retour=$?
    if [ "${V_N_Code_Retour}" -ne "0" ]; then
      f_Log "ATTENTION : etiquette non modifiee (code ${V_N_Code_Retour}), roue linux generique conservee"
    fi
  fi
fi

f_Log "Roue(s) produite(s) :"
ls -1 "${V_Vc_Rep_Sortie}"/hexbandit-*.whl
exit 0
