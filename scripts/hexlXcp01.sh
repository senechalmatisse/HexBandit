#!/bin/bash
#
# FONCTION : Lancement (ou reprise) d'une campagne d'experiences hexbandit puis
#            generation du rapport (graphiques G1 a G5, statistiques).
#            Script fonctionnel : la campagne reprend la ou elle s'etait
#            arretee, les tranches deja jouees ne sont pas rejouees.
#
# PARAMETRES : Ce programme prend en parametre
#              1: le fichier TOML de la campagne
#              2: le repertoire des resultats
# (facultatif) 3: la duree maximale d'execution en secondes (arret propre)
# (facultatif) 4: le nombre de replicats bootstrap du rapport (1000 par defaut)
#

V_Vc_Nom_Script="hexlXcp01"
V_Vc_Rep_Script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
V_Vc_Rep_Racine="$(cd "${V_Vc_Rep_Script}/.." && pwd)"
V_Vc_Fic_Campagne="${1}"
V_Vc_Rep_Resultats="${2}"
V_Vc_Duree_Max="${3}"
V_Vc_Nb_Bootstrap="${4:-1000}"
V_N_Code_Retour=0

. "${V_Vc_Rep_Script}/hexlXtc00.sh"

# Controle des parametres
if [ -z "${V_Vc_Fic_Campagne}" ] || [ -z "${V_Vc_Rep_Resultats}" ]; then
  f_Log "ERREUR : usage : hexlXcp01.sh <campagne.toml> <repertoire_resultats> [duree_max_s] [bootstrap]"
  exit 2
fi
if [ ! -f "${V_Vc_Fic_Campagne}" ]; then
  f_Log "ERREUR : fichier de campagne introuvable : ${V_Vc_Fic_Campagne}"
  exit 2
fi

V_Vc_Fic_Campagne="$(cd "$(dirname "${V_Vc_Fic_Campagne}")" && pwd)/$(basename "${V_Vc_Fic_Campagne}")"
mkdir -p "${V_Vc_Rep_Resultats}"
V_Vc_Rep_Resultats="$(cd "${V_Vc_Rep_Resultats}" && pwd)"

f_Python_Choisir
f_Python_Controler numpy pandas scipy matplotlib pydantic
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  exit 3
fi
f_Log "Interpreteur Python : ${V_Vc_Python}"

cd "${V_Vc_Rep_Racine}/python" || exit 2

# Estimation du cout (refus au-dela du plafond de la campagne)
f_Log "Estimation du cout de ${V_Vc_Fic_Campagne}"
"${V_Vc_Python}" -m hexbandit estimate "${V_Vc_Fic_Campagne}"
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  f_Log "ERREUR : campagne refusee (code ${V_N_Code_Retour})"
  exit "${V_N_Code_Retour}"
fi

# Execution ou reprise
f_Log "Execution de la campagne vers ${V_Vc_Rep_Resultats}"
if [ -n "${V_Vc_Duree_Max}" ]; then
  "${V_Vc_Python}" -m hexbandit run-campaign "${V_Vc_Fic_Campagne}" --results "${V_Vc_Rep_Resultats}" --time-limit "${V_Vc_Duree_Max}"
else
  "${V_Vc_Python}" -m hexbandit run-campaign "${V_Vc_Fic_Campagne}" --results "${V_Vc_Rep_Resultats}"
fi
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -eq "5" ]; then
  f_Log "Campagne incomplete (limite de temps) : relancer ce script pour reprendre"
  exit 5
fi
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  f_Log "ERREUR : echec de la campagne (code ${V_N_Code_Retour})"
  exit "${V_N_Code_Retour}"
fi

# Rapport
f_Log "Generation du rapport (bootstrap ${V_Vc_Nb_Bootstrap})"
"${V_Vc_Python}" -m hexbandit report "${V_Vc_Rep_Resultats}" --bootstrap "${V_Vc_Nb_Bootstrap}"
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  f_Log "ERREUR : echec du rapport (code ${V_N_Code_Retour})"
  exit "${V_N_Code_Retour}"
fi

f_Log "Campagne et rapport termines : ${V_Vc_Rep_Resultats}/rapport/resultats.md"
exit 0
