#!/bin/bash
#
# FONCTION : Controles qualite locaux (equivalent de la CI) : normes des
#            scripts shell (hexlXtc05.sh), formatage, clippy, tests Rust,
#            compilation de la bibliotheque native, ruff, mypy strict et pytest.
#            L'interpreteur Python est celui de HEXBANDIT_PYTHON, sinon de
#            l'environnement virtuel actif, sinon de .venv (hexlXtc03.sh), sinon python3.
#
# PARAMETRES : Ce programme prend en parametre
# (facultatif) 1: la toolchain Rust a utiliser (sinon RUSTUP_TOOLCHAIN,
#                 sinon celle de rust-toolchain.toml)
#

V_Vc_Nom_Script="hexlXtc02"
V_Vc_Rep_Script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
V_Vc_Rep_Racine="$(cd "${V_Vc_Rep_Script}/.." && pwd)"
V_Vc_Toolchain="${1:-${RUSTUP_TOOLCHAIN}}"
V_N_Nb_Echecs=0
V_N_Code_Retour=0

. "${V_Vc_Rep_Script}/hexlXtc00.sh"

# Execution d'une etape de controle : libelle puis commande et arguments
f_Etape()
{
  P_E_Vc_Libelle="${1}"
  shift
  f_Log "Debut : ${P_E_Vc_Libelle}"
  "$@"
  V_N_Code_Retour=$?
  if [ "${V_N_Code_Retour}" -eq "0" ]; then
    f_Log "OK    : ${P_E_Vc_Libelle}"
  else
    f_Log "ECHEC : ${P_E_Vc_Libelle} (code ${V_N_Code_Retour})"
    (( V_N_Nb_Echecs = V_N_Nb_Echecs + 1 ))
  fi
}

if [ -n "${V_Vc_Toolchain}" ]; then
  export RUSTUP_TOOLCHAIN="${V_Vc_Toolchain}"
fi

cd "${V_Vc_Rep_Racine}" || exit 2

f_Etape "normes des scripts shell" bash "${V_Vc_Rep_Script}/hexlXtc05.sh"
f_Etape "cargo fmt --check" cargo fmt --all --check
f_Etape "cargo clippy -D warnings" cargo clippy --workspace --all-targets -- -D warnings
f_Etape "cargo test" cargo test --workspace
f_Etape "bibliotheque native" bash "${V_Vc_Rep_Script}/hexlXtc01.sh" release

f_Python_Choisir
f_Log "Interpreteur Python : ${V_Vc_Python}"
V_Tab_Modules=(numpy pandas scipy matplotlib pydantic ruff mypy pytest)
V_Tab_Ruff=("${V_Vc_Python}" -m ruff)
if ! "${V_Vc_Python}" -c "import ruff" > /dev/null 2>&1 && command -v ruff > /dev/null 2>&1; then
  # ruff installe comme executable autonome (uv tool, pipx, binaire)
  f_Log "ruff : executable $(command -v ruff)"
  V_Tab_Modules=(numpy pandas scipy matplotlib pydantic mypy pytest)
  V_Tab_Ruff=(ruff)
fi
f_Python_Controler "${V_Tab_Modules[@]}"
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -eq "0" ]; then
  cd "${V_Vc_Rep_Racine}/python" || exit 2
  f_Etape "ruff" "${V_Tab_Ruff[@]}" check hexbandit tests
  f_Etape "mypy strict" "${V_Vc_Python}" -m mypy
  f_Etape "pytest" "${V_Vc_Python}" -m pytest -q
else
  f_Log "ECHEC : controles Python (ruff, mypy, pytest) non lances"
  (( V_N_Nb_Echecs = V_N_Nb_Echecs + 1 ))
fi

if [ "${V_N_Nb_Echecs}" -eq "0" ]; then
  f_Log "Tous les controles sont passes"
  exit 0
fi
f_Log "ERREUR : ${V_N_Nb_Echecs} controle(s) en echec"
exit 1
