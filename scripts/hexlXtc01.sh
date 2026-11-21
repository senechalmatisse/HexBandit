#!/bin/bash
#
# FONCTION : Compilation de la bibliotheque native hexbandit (crate bindings)
#            et installation dans le paquet Python (python/hexbandit/_lib)
#
# PARAMETRES : Ce programme prend en parametre
# (facultatif) 1: le profil Cargo : release (defaut) ou debug
# (facultatif) 2: la toolchain Rust a utiliser (sinon RUSTUP_TOOLCHAIN,
#                 sinon celle de rust-toolchain.toml)
#

V_Vc_Nom_Script="hexlXtc01"
V_Vc_Rep_Script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
V_Vc_Rep_Racine="$(cd "${V_Vc_Rep_Script}/.." && pwd)"
V_Vc_Profil="${1:-release}"
V_Vc_Toolchain="${2:-${RUSTUP_TOOLCHAIN}}"
V_Vc_Rep_Cible="${V_Vc_Rep_Racine}/python/hexbandit/_lib"
V_Vc_Nom_Lib=""
V_N_Code_Retour=0

. "${V_Vc_Rep_Script}/hexlXtc00.sh"

# Controle des parametres
if [ "${V_Vc_Profil}" != "release" ] && [ "${V_Vc_Profil}" != "debug" ]; then
  f_Log "ERREUR : profil inconnu ${V_Vc_Profil} (release ou debug)"
  exit 2
fi

case "$(uname -s)" in
  Linux*)
    V_Vc_Nom_Lib="libhexbandit_native.so"
    ;;
  Darwin*)
    V_Vc_Nom_Lib="libhexbandit_native.dylib"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    V_Vc_Nom_Lib="hexbandit_native.dll"
    ;;
  *)
    f_Log "ERREUR : systeme non reconnu $(uname -s)"
    exit 2
    ;;
esac

if [ -n "${V_Vc_Toolchain}" ]; then
  export RUSTUP_TOOLCHAIN="${V_Vc_Toolchain}"
  f_Log "Toolchain Rust imposee : ${RUSTUP_TOOLCHAIN}"
fi

# Compilation
f_Log "Compilation du crate bindings (profil ${V_Vc_Profil})"
if [ "${V_Vc_Profil}" = "release" ]; then
  cargo build --release -p bindings --manifest-path "${V_Vc_Rep_Racine}/Cargo.toml"
else
  cargo build -p bindings --manifest-path "${V_Vc_Rep_Racine}/Cargo.toml"
fi
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  f_Log "ERREUR : echec de la compilation (code ${V_N_Code_Retour})"
  exit "${V_N_Code_Retour}"
fi

# Installation dans le paquet Python
#
#
# Copie dans un fichier temporaire puis renommage : un processus qui a deja
# charge l'ancienne bibliotheque continue de l'utiliser sans risque.
mkdir -p "${V_Vc_Rep_Cible}"
cp "${V_Vc_Rep_Racine}/target/${V_Vc_Profil}/${V_Vc_Nom_Lib}" "${V_Vc_Rep_Cible}/${V_Vc_Nom_Lib}.tmp" \
  && mv -f "${V_Vc_Rep_Cible}/${V_Vc_Nom_Lib}.tmp" "${V_Vc_Rep_Cible}/${V_Vc_Nom_Lib}"
V_N_Code_Retour=$?
if [ "${V_N_Code_Retour}" -ne "0" ]; then
  f_Log "ERREUR : copie impossible vers ${V_Vc_Rep_Cible} (code ${V_N_Code_Retour})"
  exit "${V_N_Code_Retour}"
fi

f_Log "Bibliotheque installee : ${V_Vc_Rep_Cible}/${V_Vc_Nom_Lib}"
exit 0
