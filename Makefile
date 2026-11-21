# Makefile de hexbandit : raccourcis pour installer, jouer, tester et reproduire
# les experiences. Les cibles appellent les scripts de scripts/ et la ligne de
# commande python -m hexbandit ; rien n'est fait ici qui ne soit faisable a la main.
#
# make            liste les cibles (identique a make aide)
# make demo       une partie IA contre IA : image, animation et visionneuse web
#
# Parametres (a passer sur la ligne de commande : make partie TAILLE=13) :
#   TAILLE, BLEU, ROUGE, JEU    partie (taille 2 a 19 ; joueurs algo:simulations,options)
#   A, B, PARTIES               match A contre B
#   CAMPAGNE, RESULTATS, DUREE  campagne TOML, repertoire de resultats, duree max (s)
#   THREADS                     threads de calcul (2 par defaut)
#   TOOLCHAIN                   toolchain Rust (ex. stable ; sinon rust-toolchain.toml)
#   PYTHON                      interpreteur (sinon HEXBANDIT_PYTHON, venv actif, .venv, python3)
#
# Les recettes sont indentees par des tabulations, comme make l'exige ; les
# scripts shell du projet restent sans tabulation.

SHELL := /bin/bash
.DEFAULT_GOAL := aide
RACINE := $(CURDIR)

TAILLE ?= 11
JEU ?= hex
BLEU ?= rave:2000
ROUGE ?= uct:2000,c=0.25
A ?= rave:1000,prior=hex,grave=50
B ?= rave:1000
PARTIES ?= 200
THREADS ?= 2
SORTIE ?= demo
CAMPAGNE ?= experiments/campagne_reduite.toml
RESULTATS ?= results/reduite
DUREE ?=
TOOLCHAIN ?=

ifneq ($(strip $(TOOLCHAIN)),)
export RUSTUP_TOOLCHAIN := $(TOOLCHAIN)
endif

# Interpreteur Python : meme ordre que les scripts (hexlXtc00.sh).
PYTHON ?= $(shell \
  if [ -n "$$HEXBANDIT_PYTHON" ]; then echo "$$HEXBANDIT_PYTHON"; \
  elif [ -n "$$VIRTUAL_ENV" ] && [ -x "$$VIRTUAL_ENV/bin/python" ]; then echo "$$VIRTUAL_ENV/bin/python"; \
  elif [ -x "$(RACINE)/.venv/bin/python" ]; then echo "$(RACINE)/.venv/bin/python"; \
  elif [ -x "$(RACINE)/.venv/Scripts/python.exe" ]; then echo "$(RACINE)/.venv/Scripts/python.exe"; \
  else echo python3; fi)
HEXBANDIT = cd "$(RACINE)/python" && "$(PYTHON)" -m hexbandit
SCRIPT = HEXBANDIT_PYTHON="$(PYTHON)" bash "$(RACINE)/scripts/$(1)"

# Bibliotheque native : reconstruite seulement si une source Rust a change.
SYSTEME := $(shell uname -s 2>/dev/null)
ifeq ($(SYSTEME),Darwin)
LIB_NOM := libhexbandit_native.dylib
else ifneq ($(filter MINGW% MSYS% CYGWIN%,$(SYSTEME)),)
LIB_NOM := hexbandit_native.dll
else
LIB_NOM := libhexbandit_native.so
endif
LIB := python/hexbandit/_lib/$(LIB_NOM)
SOURCES_RUST := $(shell find crates -name '*.rs' -o -name Cargo.toml) Cargo.toml Cargo.lock

.PHONY: aide help installer install lib build demo partie match visionneuse \
  tournoi verifier check qa test format normes bench roue estimer campagne \
  rapport determinisme gateau adaptatif validation nettoyer clean nettoyer-tout

##@ Pour commencer

aide: ## liste des cibles (cible par defaut)
	@awk 'BEGIN {FS = ":.*## "} /^##@/ {printf "\n\033[1m%s\033[0m\n", substr($$0, 5)} /^[a-z][a-z0-9-]*:.*## / {printf "  make %-15s %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf '\nParametres : make partie TAILLE=13 BLEU=rave:5000 ROUGE=uct:5000 ; make match A=... B=... PARTIES=500\n'
	@printf 'Alias anglais : help install build demo check test bench clean\n'
	@printf 'Interpreteur Python : %s\n' "$(PYTHON)"

help: aide

installer: ## cree .venv, y installe hexbandit et ses dependances (acces PyPI), compile le moteur
	$(call SCRIPT,hexlXtc01.sh)
	$(call SCRIPT,hexlXtc03.sh)
	$(MAKE) --no-print-directory lib

install: installer

lib: $(LIB) ## compile la bibliotheque native Rust (si une source a change)

build: lib

$(LIB): $(SOURCES_RUST)
	bash scripts/hexlXtc01.sh release

demo: lib ## une partie IA contre IA en image, en animation et dans la visionneuse web
	@mkdir -p "$(SORTIE)"
	$(HEXBANDIT) play --kind $(JEU) --size $(TAILLE) --blue $(BLEU) --red $(ROUGE) --report full \
	  --out "$(RACINE)/$(SORTIE)/partie.json" --image "$(RACINE)/$(SORTIE)/partie.png" --gif "$(RACINE)/$(SORTIE)/partie.gif"
	$(HEXBANDIT) view "$(RACINE)/$(SORTIE)/partie.json" --out "$(RACINE)/$(SORTIE)/visionneuse.html"
	@printf '\nA ouvrir : %s/partie.gif et %s/visionneuse.html (rejeu coup par coup, cartes de chaleur)\n' "$(SORTIE)" "$(SORTIE)"

##@ Jouer et comparer

partie: lib ## une partie (TAILLE, BLEU, ROUGE, JEU=hex|reverse_hex|chameleon|quarto) en JSON, et en PNG sur plateau de Hex
	@mkdir -p "$(SORTIE)"
	$(HEXBANDIT) play --kind $(JEU) --size $(TAILLE) --blue $(BLEU) --red $(ROUGE) --report full \
	  --out "$(RACINE)/$(SORTIE)/partie.json" --image "$(RACINE)/$(SORTIE)/partie.png"

match: lib ## match A contre B (PARTIES, TAILLE) : taux de victoire, IC de Wilson, test binomial
	$(HEXBANDIT) match --kind $(JEU) --size $(TAILLE) --a $(A) --b $(B) --games $(PARTIES) --threads $(THREADS)

tournoi: lib ## tournoi toutes rondes 9 x 9 (UCT, MC-RAVE, options) et classement Elo
	$(HEXBANDIT) tournament ../experiments/tournoi_9x9.toml --threads $(THREADS) --out ../results/tournoi_9x9

visionneuse: ## ouvre la visionneuse d'exemple (Hex, solveur, Chameleon) dans le navigateur
	"$(PYTHON)" -m webbrowser "file://$(RACINE)/docs/figures/visionneuse.html"

##@ Qualite

verifier: ## tous les controles de la CI : normes shell, fmt, clippy, tests Rust, ruff, mypy, pytest
	$(call SCRIPT,hexlXtc02.sh)

check: verifier

qa: verifier

test: lib ## tests Rust et Python seulement
	cargo test --workspace
	cd python && "$(PYTHON)" -m pytest -q

format: ## formate le code Rust (cargo fmt) et Python (ruff format)
	cargo fmt --all
	cd python && (ruff format hexbandit tests 2>/dev/null || "$(PYTHON)" -m ruff format hexbandit tests)

normes: ## controle des normes des scripts shell
	bash scripts/hexlXtc05.sh

bench: ## banc de performance du moteur (simulations par seconde, 14 x 14)
	cargo bench -p mcts

roue: ## roue Python py3-none-<plateforme> dans dist/
	bash scripts/hexlXtc04.sh dist

##@ Experiences

estimer: ## cout estime d'une campagne (CAMPAGNE)
	$(HEXBANDIT) estimate "$(RACINE)/$(CAMPAGNE)"

campagne: lib ## joue ou reprend une campagne puis produit son rapport (CAMPAGNE, RESULTATS, DUREE)
	$(call SCRIPT,hexlXcp01.sh) "$(RACINE)/$(CAMPAGNE)" "$(RACINE)/$(RESULTATS)" $(DUREE)

rapport: ## rapport d'une campagne : resultats.md, figures G1 a G7, CSV (RESULTATS)
	$(HEXBANDIT) report "$(RACINE)/$(RESULTATS)"

determinisme: lib ## rejoue 6 tranches d'une campagne et verifie qu'elles sont identiques
	$(HEXBANDIT) verify "$(RACINE)/$(CAMPAGNE)" --results "$(RACINE)/$(RESULTATS)" --chunks 6 --threads 1

gateau: lib ## campagne avec la regle du gateau, puis son rapport
	$(HEXBANDIT) run-campaign ../experiments/campagne_gateau.toml --results ../results/gateau --threads $(THREADS)
	$(HEXBANDIT) report ../results/gateau

adaptatif: lib ## plan adaptatif de r* (methode Psi), compare a la campagne complete
	$(HEXBANDIT) adaptive ../experiments/adaptatif_rstar.toml --results ../results/adaptatif \
	  --threads $(THREADS) --compare ../results/complete --source ../results/reduite

validation: lib ## matchs de validation et des options
	$(call SCRIPT,hexlXcp02.sh) "$(RACINE)/experiments/validation" $(THREADS)
	$(call SCRIPT,hexlXcp03.sh) "$(RACINE)/experiments/validation" $(THREADS)

##@ Menage

nettoyer: ## supprime les sorties de demonstration, roues et caches Python (jamais results/)
	rm -rf demo dist build python/build
	find python -name __pycache__ -type d -prune -exec rm -rf {} +
	rm -rf python/.mypy_cache python/.pytest_cache python/.ruff_cache

clean: nettoyer

nettoyer-tout: nettoyer ## en plus, supprime target/ et la bibliotheque native
	cargo clean
	rm -f "$(LIB)"
