# The faces this crate ships

Each is the face Excalidraw draws with, rebuilt from the per-range subsets its own repository
publishes and merged back into one file. None has been changed apart from that.

| File | Face | Licence |
|---|---|---|
| `Excalifont-Regular.ttf` | Excalifont | SIL Open Font License 1.1 |
| `Nunito-Regular.ttf` | Nunito | SIL Open Font License 1.1 |
| `ComicShanns-Regular.ttf` | Comic Shanns | MIT |

Excalifont is by Ján Filípek / DizajnDesign, after Virgil by Your Own Font Foundry.
Nunito is by Vernon Adams, Cyreal and Jacques Le Bailly.
Comic Shanns is by Shannon Miwa, with later work by Jesus Gonzalez, Rodrigo Batista de Moraes,
Fini Jastrow and Kyle Beechly.

The full text of the SIL Open Font License 1.1 is at <https://openfontlicense.org>.

## Rebuilding

```sh
base=https://raw.githubusercontent.com/excalidraw/excalidraw/master/packages/excalidraw/fonts
curl -sSLO "$base/Excalifont/Excalifont-Regular-<hash>.woff2"   # every subset of the face
woff2_decompress Excalifont-Regular-<hash>.woff2
fonttools merge --output-file=Excalifont-Regular.ttf Excalifont-Regular-*.ttf
```
