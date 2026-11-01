# irblaster-data
Public firmware and IR code catalog for the IR Blaster hub

Served by GitHub Pages at <https://olkham.github.io/irblaster-data/>.

| Path | What it is |
|---|---|
| `firmware/manifest.json`, `firmware/*.bin` | current node and hub images, written by `tools/publish_release.py` |
| `catalog/` | the IR code catalog read by the hub's *Add device* page |
| `index.html`, `assets/` | the public landing page |

The hub and the nodes fetch `firmware/` and `catalog/` by these exact paths, so don't move or rename
them. The landing page builds its firmware tables from `firmware/manifest.json` in the browser, so a
new publish needs no change to it. Its guide links are placeholders: when a guide is published at the
path its link points to (`docs/<name>/`), remove `data-soon` from that link in `index.html`.
