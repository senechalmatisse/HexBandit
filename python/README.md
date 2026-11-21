# hexbandit (paquet Python)

Pilotage, analyse et visualisation du moteur Hex / MCTS / MC-RAVE écrit en Rust. Voir le `README.md` à la racine du dépôt pour l'architecture, la construction de la bibliothèque native et la reproduction des résultats.

Depuis la racine du dépôt :

```sh
bash scripts/hexlXtc01.sh               # compile et installe la bibliothèque native
bash scripts/hexlXtc03.sh               # environnement virtuel .venv (Python >= 3.11)
source .venv/bin/activate
python -m hexbandit play --size 11 --blue rave:2000 --red uct:2000,c=0.25 --gif partie.gif
python -m hexbandit run-campaign experiments/campagne_reduite.toml --results results/reduite
python -m hexbandit report results/reduite
```
