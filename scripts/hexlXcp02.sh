#!/bin/bash
#
# FONCTION : Matchs de validation du moteur : reglage de MC-RAVE et de UCT,
#            critere d'acceptation (MC-RAVE contre UCT a 3000 simulations
#            par coup), controle budget minimal, Reverse Hex et Hex sur
#            petits plateaux. Script fonctionnel.
#
# PARAMETRES : Ce programme prend en parametre
# (facultatif) 1: le repertoire de sortie des bilans JSON
#                 (experiments/validation par defaut)
# (facultatif) 2: le nombre de threads (2 par defaut)
#

V_Vc_Nom_Script="hexlXcp02"
V_Vc_Rep_Script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
V_Vc_Rep_Racine="$(cd "${V_Vc_Rep_Script}/.." && pwd)"
V_Vc_Rep_Sortie="${1:-${V_Vc_Rep_Racine}/experiments/validation}"
V_N_Nb_Threads="${2:-2}"
V_N_Nb_Echecs=0
V_N_Code_Retour=0
V_N_Taille=0
V_Vc_Valeur=""

. "${V_Vc_Rep_Script}/hexlXtc00.sh"

# Match A contre B : fichier de sortie puis arguments de hexbandit match
f_Match()
{
  P_E_Vc_Fichier="${1}"
  shift
  "${V_Vc_Python}" -m hexbandit match "$@" --threads "${V_N_Nb_Threads}" --out "${V_Vc_Rep_Sortie}/${P_E_Vc_Fichier}"
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
cd "${V_Vc_Rep_Racine}/python" || exit 2

f_Log "Reglage de MC-RAVE (reference : planning manuel k=1000)"
for V_Vc_Valeur in 0 0.02 0.05 0.1 0.2; do
  f_Match "reglage_rave.json" --size 9 --a "rave:1000,beta=mse,bias=${V_Vc_Valeur},label=rave-mse-b${V_Vc_Valeur}" \
    --b "rave:1000,beta=manual,k=1000,label=ref-k1000" --games 200 --seed 77
done
for V_Vc_Valeur in 300 3000; do
  f_Match "reglage_rave.json" --size 9 --a "rave:1000,beta=manual,k=${V_Vc_Valeur},label=rave-manual-k${V_Vc_Valeur}" \
    --b "rave:1000,beta=manual,k=1000,label=ref-k1000" --games 200 --seed 77
done

f_Log "Reglage de UCT (reference : c=racine de 2)"
for V_Vc_Valeur in 0.25 0.5 1.0; do
  f_Match "reglage_uct.json" --size 9 --a "uct:1000,c=${V_Vc_Valeur},label=uct-c${V_Vc_Valeur}" \
    --b "uct:1000,label=uct-c1.414" --games 200 --seed 78
done

f_Log "Critere d'acceptation : MC-RAVE contre UCT a 3000 simulations par coup"
for V_N_Taille in 7 9 11; do
  f_Match "acceptation.json" --size "${V_N_Taille}" --a "rave:3000,label=rave-3000" \
    --b "uct:3000,c=0.25,label=uct-3000-c0.25" --games 200 --seed "300${V_N_Taille}"
done
f_Match "acceptation.json" --size 14 --a "rave:3000,label=rave-3000" \
  --b "uct:3000,c=0.25,label=uct-3000-c0.25" --games 100 --seed 30014

f_Log "Controle budget minimal (jeu quasi aleatoire)"
for V_N_Taille in 5 7 9 11 14; do
  f_Match "controle_budget_minimal.json" --size "${V_N_Taille}" --a "uct:1,label=uct-1" --b "uct:1,label=uct-1" \
    --games 2000 --seed "400${V_N_Taille}" --seating a_first_alternate_colors
done

f_Log "Reverse Hex : premier joueur gagnant si N pair (Lagarias et Sleator)"
for V_N_Taille in 3 4 5 6; do
  f_Match "reverse_hex.json" --kind reverse_hex --size "${V_N_Taille}" --a "rave:20000,label=rave-20000" \
    --b "rave:20000,label=rave-20000" --games 60 --seed "500${V_N_Taille}" --seating a_first_alternate_colors
done

if [ "${V_N_Nb_Echecs}" -eq "0" ]; then
  f_Log "Validation terminee : bilans dans ${V_Vc_Rep_Sortie}"
  exit 0
fi
f_Log "ERREUR : ${V_N_Nb_Echecs} match(s) en echec"
exit 1
