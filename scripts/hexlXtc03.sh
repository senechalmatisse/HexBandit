#!/bin/bash
#
# FONCTION : Creation (ou mise a jour) de l'environnement virtuel Python du
#            projet et installation du paquet hexbandit en mode editable avec
#            ses dependances (numpy, pandas, scipy, matplotlib, pydantic et
#            les outils de developpement). Necessite un acces a PyPI.
#
# PARAMETRES : Ce programme prend en parametre
# (facultatif) 1: l'interpreteur Python de base (python3 par defaut, version 3.11 minimum)
# (facultatif) 2: les extras a installer, separes par des virgules
#                 (dev par defaut ; dev,parquet,crosscheck pour tout ;
#                 aucun pour les seules dependances d'execution)
# (facultatif) 3: le repertoire de l'environnement (.venv a la racine du depot par defaut)
#
#              Variables d'environnement facultatives :
#              HEXBANDIT_VENV_OPTIONS : options supplementaires de python -m venv
#                                    (exemple : --system-site-packages)
#              HEXBANDIT_PIP_OPTIONS  : options supplementaires de pip install
#                                    (exemple : --index-url d'un miroir)
#

V_Vc_Nom_Script="hexlXtc03"
V_Vc_Rep_Script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
V_Vc_Rep_Racine="$(cd "${V_Vc_Rep_Script}/.." && pwd)"
V_Vc_Python_Base="${1:-python3}"
V_Vc_Extras="${2:-dev}"
V_Vc_Rep_Venv="${3:-${V_Vc_Rep_Racine}/.venv}"
V_Vc_Version=""
V_Vc_Lib_Native=""
V_Vc_Cible_Pip=""
V_N_Code_Retour=0
V_Tab_Options_Venv=()
V_Tab_Options_Pip=()

. "${V_Vc_Rep_Script}/hexlXtc00.sh"

read -r -a V_Tab_Options_Venv <<< "${HEXBANDIT_VENV_OPTIONS}"
read -r -a V_Tab_Options_Pip <<< "${HEXBANDIT_PIP_OPTIONS}"

# Controle de l'interpreteur de base
command -v "${V_Vc_Python_Base}" > /dev/null 2>&1
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  f_Log "ERREUR : interpreteur introuvable : ${V_Vc_Python_Base}"
  exit 2
fi
V_Vc_Version="$("${V_Vc_Python_Base}" -c 'import sys; print("%d.%d" % sys.version_info[:2])')"
"${V_Vc_Python_Base}" -c 'import sys; sys.exit(0 if sys.version_info >= (3, 11) else 1)'
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  f_Log "ERREUR : Python ${V_Vc_Version} trop ancien : 3.11 minimum (parametre 1 pour en choisir un autre)"
  exit 2
fi

# Creation de l'environnement s'il n'existe pas encore
if [ -x "${V_Vc_Rep_Venv}/bin/python" ]; then
  f_Log "Environnement existant reutilise : ${V_Vc_Rep_Venv}"
else
  "${V_Vc_Python_Base}" -c 'import ensurepip, venv' > /dev/null 2>&1
  V_N_Code_Retour=$?
  if [ "${V_N_Code_Retour}" -ne "0" ]; then
    f_Log "ERREUR : les modules venv et ensurepip manquent a Python ${V_Vc_Version}"
    f_Log "         Debian ou Ubuntu : sudo apt install python${V_Vc_Version}-venv"
    exit 3
  fi
  f_Log "Creation de l'environnement ${V_Vc_Rep_Venv} (Python ${V_Vc_Version})"
  "${V_Vc_Python_Base}" -m venv "${V_Tab_Options_Venv[@]}" "${V_Vc_Rep_Venv}"
  V_N_Code_Retour=$?
  if [ "${V_N_Code_Retour}" -ne "0" ]; then
    f_Log "ERREUR : echec de la creation de l'environnement (code ${V_N_Code_Retour})"
    f_Log "         Debian ou Ubuntu : sudo apt install python${V_Vc_Version}-venv"
    exit 3
  fi
fi
V_Vc_Python="${V_Vc_Rep_Venv}/bin/python"

# Verification et initialisation de pip
if ! "${V_Vc_Python}" -m pip --version > /dev/null 2>&1; then
  f_Log "pip absent : tentative d'initialisation avec ensurepip"
  if ! "${V_Vc_Python}" -m ensurepip --upgrade; then
    f_Log "ERREUR : impossible d'initialiser pip dans l'environnement"
    f_Log "         Debian ou Ubuntu : sudo apt install python${V_Vc_Version}-venv"
    exit 4
  fi
fi

# Mise a jour de pip
f_Log "Mise a jour de pip"
"${V_Vc_Python}" -m pip install --quiet --upgrade "${V_Tab_Options_Pip[@]}" pip
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  f_Log "ATTENTION : mise a jour de pip impossible (code ${V_N_Code_Retour})"
  f_Log "            poursuite avec la version presente"
fi

# Installation du paquet et de ses dependances
if [ "${V_Vc_Extras}" = "aucun" ]; then
  V_Vc_Cible_Pip="${V_Vc_Rep_Racine}/python"
else
  V_Vc_Cible_Pip="${V_Vc_Rep_Racine}/python[${V_Vc_Extras}]"
fi
f_Log "Installation de ${V_Vc_Cible_Pip} en mode editable"
"${V_Vc_Python}" -m pip install "${V_Tab_Options_Pip[@]}" -e "${V_Vc_Cible_Pip}"
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  f_Log "ERREUR : echec de l'installation (code ${V_N_Code_Retour})"
  exit 4
fi

# Controles
f_Python_Controler hexbandit numpy pandas scipy matplotlib pydantic
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  exit 4
fi

V_Vc_Lib_Native="$(find "${V_Vc_Rep_Racine}/python/hexbandit/_lib" -maxdepth 1 -name '*hexbandit_native*' 2> /dev/null | head -n 1)"
if [ -z "${V_Vc_Lib_Native}" ]; then
  f_Log "ATTENTION : bibliotheque native absente : lancer bash scripts/hexlXtc01.sh"
fi

"${V_Vc_Python}" -m hexbandit --version

f_Log "Environnement pret : ${V_Vc_Rep_Venv}"
f_Log "Pour l'utiliser dans le terminal : source ${V_Vc_Rep_Venv}/bin/activate"
exit 0
