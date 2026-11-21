#!/bin/bash
#
# FONCTION : Fonctions communes aux scripts hexbandit : journal horodate,
#            choix de l'interpreteur Python et controle de ses modules.
#            Fichier a sourcer par les autres scripts, il ne s'execute pas seul.
#
# PARAMETRES : Aucun. Variables lues :
#              V_Vc_Nom_Script : nom court du script appelant (journal)
#              V_Vc_Rep_Racine : racine du depot
#              HEXBANDIT_PYTHON   : interpreteur Python impose (facultatif)
#              VIRTUAL_ENV     : environnement virtuel actif (facultatif)
#

if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
  echo "hexlXtc00.sh contient des fonctions communes : il se source depuis un autre script" >&2
  exit 2
fi

V_Vc_Python=""
V_Vc_Modules_Absents=""
V_N_Code_Controle=0

# Affichage horodate d'un message
f_Log()
{
  P_E_Vc_Message="${1}"
  echo "$(date +%Y-%m-%dT%H:%M:%S) [${V_Vc_Nom_Script}] ${P_E_Vc_Message}"
}

# Choix de l'interpreteur Python (resultat dans V_Vc_Python), par priorite :
#   1. HEXBANDIT_PYTHON s'il est defini
#   2. l'environnement virtuel actif (VIRTUAL_ENV)
#   3. l'environnement virtuel du projet (.venv, cree par hexlXtc03.sh)
#   4. python3
f_Python_Choisir()
{
  if [ -n "${HEXBANDIT_PYTHON}" ]; then
    V_Vc_Python="${HEXBANDIT_PYTHON}"
  elif [ -n "${VIRTUAL_ENV}" ] && [ -x "${VIRTUAL_ENV}/bin/python" ]; then
    V_Vc_Python="${VIRTUAL_ENV}/bin/python"
  elif [ -x "${V_Vc_Rep_Racine}/.venv/bin/python" ]; then
    V_Vc_Python="${V_Vc_Rep_Racine}/.venv/bin/python"
  else
    V_Vc_Python="python3"
  fi
}

# Controle que V_Vc_Python trouve chacun des modules passes en parametres
# (noms de modules Python). En cas d'absence, indique comment creer
# l'environnement. Code retour : 0 si tout est present, 1 sinon.
f_Python_Controler()
{
  V_Vc_Modules_Absents="$("${V_Vc_Python}" -c 'import importlib.util, sys; print(" ".join(m for m in sys.argv[1:] if importlib.util.find_spec(m) is None))' "$@" 2>&1)"
  V_N_Code_Controle=$?
  if [ "${V_N_Code_Controle}" -ne "0" ]; then
    f_Log "ERREUR : interpreteur Python inutilisable : ${V_Vc_Python} (${V_Vc_Modules_Absents})"
    f_Log "         creer l'environnement du projet : bash scripts/hexlXtc03.sh"
    return 1
  fi
  if [ -n "${V_Vc_Modules_Absents}" ]; then
    f_Log "ERREUR : modules Python absents pour ${V_Vc_Python} : ${V_Vc_Modules_Absents}"
    f_Log "         creer l'environnement du projet : bash scripts/hexlXtc03.sh"
    return 1
  fi
  return 0
}
