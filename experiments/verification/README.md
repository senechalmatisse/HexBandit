# Tranches de vérification

Six tranches de 100 parties de la campagne réduite (`results/reduite`), versionnées pour la CI : `hexbandit verify` les rejoue sur Linux, macOS et Windows et vérifie que les parties (graine, couleur du premier joueur, vainqueur, nombre de coups).

```sh
cd python
python -m hexbandit verify ../experiments/campagne_reduite.toml --results ../experiments/verification/reduite --chunks 6 --threads 1
```
