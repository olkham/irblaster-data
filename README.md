# irblaster-data
Public firmware and IR code catalog for the IR Blaster hub

Served by GitHub Pages at <https://olkham.github.io/irblaster-data/>.

| Path | What it is |
|---|---|
| `firmware/manifest.json`, `firmware/*.bin` | current node and hub images, written by `tools/publish_release.py` |
| `firmware/boot/` | bootloader, partition table and OTA data for USB recovery, written by `recovery/scripts/make_boot.py` |
| `catalog/` | the IR code catalog read by the hub's *Add device* page |
| `index.html`, `assets/` | the public landing page |
| `recovery/` | source of the USB recovery app; its builds are published as `recovery-v*` releases ([recovery/README.md](recovery/README.md)) |

The hub and the nodes fetch `firmware/` and `catalog/` by these exact paths, so don't move or rename
them. The landing page builds its firmware tables from `firmware/manifest.json` in the browser, so a
new publish needs no change to it. Its guide links are placeholders: when a guide is published at the
path its link points to (`docs/<name>/`), remove `data-soon` from that link in `index.html`.

`publish_release.py` only replaces `firmware/irblaster-*.bin` and `firmware/manifest.json`, so
`firmware/boot/` survives a publish. Refresh it when the firmware's partition tables or Arduino core
change (see [recovery/README.md](recovery/README.md)).
