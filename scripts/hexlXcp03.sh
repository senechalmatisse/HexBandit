#!/bin/bash
#
# FONCTION : Matchs de validation des options : connaissance a
#            priori du Hex, GRAVE (seuils de 5 a 200), MCTS-Solver,
#            reutilisation de l'arbre, parallelisme a la racine et choix du
#            coup final, chacune contre MC-RAVE de reference (b = 0,02) a
#            budget egal, en 9 x 9, puis en 14 x 14 pour la connaissance a
#            priori et la reutilisation ; Chameleon (MC-RAVE contre UCT).
#            Les bilans JSON sont recrees a chaque lancement. Script
#            fonctionnel (environ 20 minutes sur 2 coeurs avec 1000 parties par match).
#
# PARAMETRES : Ce programme prend en parametre
# (facultatif) 1: le repertoire de sortie des bilans JSON
#                 (experiments/validation par defaut)
# (facultatif) 2: le nombre de threads (2 par defaut)
# (facultatif) 3: le nombre de parties par match (1000 par defaut)
#

V_Vc_Nom_Script="hexlXcp03"
V_Vc_Rep_Script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
V_Vc_Rep_Racine="$(cd "${V_Vc_Rep_Script}/.." && pwd)"
V_Vc_Rep_Sortie="${1:-${V_Vc_Rep_Racine}/experiments/validation}"
V_N_Nb_Threads="${2:-2}"
V_N_Nb_Parties="${3:-1000}"
V_N_Nb_Echecs=0
V_N_Code_Retour=0
V_Vc_Option=""
V_Vc_Reference="rave:1000,label=rave-1000"

. "${V_Vc_Rep_Script}/hexlXtc00.sh"

# Match A contre B : fichier de sortie puis arguments de hexbandit match
f_Match()
{
  P_E_Vc_Fichier="${1}"
  shift
  "${V_Vc_Python}" -m hexbandit match "$@" --threads "${V_N_Nb_Threads}" --games "${V_N_Nb_Parties}" \
    --out "${V_Vc_Rep_Sortie}/${P_E_Vc_Fichier}"
  V_N_Code_Retour=$?
  if [ "${V_N_Code_Retour}" -ne "0" ]; then
    f_Log "ECHEC du match (code ${V_N_Code_Retour}) : $*"
    (( V_N_Nb_Echecs = V_N_Nb_Echecs + 1 ))
  fi
}

f_Python_Choisir
f_Python_Controler numpy pandas scipy matplotlib pydantic
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  exit 3
fi
f_Log "Interpreteur Python : ${V_Vc_Python}"

mkdir -p "${V_Vc_Rep_Sortie}"
V_Vc_Rep_Sortie="$(cd "${V_Vc_Rep_Sortie}" && pwd)"
rm -f "${V_Vc_Rep_Sortie}/options_v2.json" "${V_Vc_Rep_Sortie}/chameleon.json"
cd "${V_Vc_Rep_Racine}/python" || exit 2

f_Log "Options complementaires contre MC-RAVE de reference, 9 x 9, 1000 simulations par coup"
for V_Vc_Option in "prior=hex" "grave=5" "grave=10" "grave=25" "grave=50" "grave=100" "grave=200" \
  "solver" "reuse" "trees=2" "final=secure" "final=max_robust"; do
  f_Match "options_v2.json" --size 9 --a "rave:1000,${V_Vc_Option},label=rave-1000-${V_Vc_Option}" \
    --b "${V_Vc_Reference}" --seed 9001
done

f_Log "Connaissance a priori et reutilisation de l'arbre en 14 x 14"
for V_Vc_Option in "prior=hex" "reuse"; do
  f_Match "options_v2.json" --size 14 --a "rave:1000,${V_Vc_Option},label=rave-1000-${V_Vc_Option}" \
    --b "${V_Vc_Reference}" --seed 9014
done

f_Log "Chameleon : MC-RAVE contre UCT, 7 x 7, 1000 simulations par coup"
f_Match "chameleon.json" --kind chameleon --size 7 --a "rave:1000,label=rave-1000" \
  --b "uct:1000,c=0.25,label=uct-1000-c0.25" --seed 9107

if [ "${V_N_Nb_Echecs}" -eq "0" ]; then
  f_Log "Validation terminee : bilans dans ${V_Vc_Rep_Sortie}"
  exit 0
fi
f_Log "ERREUR : ${V_N_Nb_Echecs} match(s) en echec"
exit 1
