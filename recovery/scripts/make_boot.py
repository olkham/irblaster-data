#!/usr/bin/env python3
"""Writes the boot set the recovery app flashes before the firmware: firmware/boot/.

A board that will not start needs more than the OTA image in firmware/: the second-stage
bootloader, the partition table and the OTA data that selects the first app slot. This script puts
those three files for every published product and chip under firmware/boot/<product>-<chip>/ and
describes them in firmware/boot/index.json, which the recovery app reads.

From a firmware build (preferred: the files are exactly what `pio run` produced):

    python recovery/scripts/make_boot.py --pio-build ../esp32-ir-blaster/firmware/.pio/build

Reconstructed from the Arduino core release instead, when no build is at hand (needs esptool 4.5.1,
the version espressif32 @ 6.9.0 uses):

    python recovery/scripts/make_boot.py --arduino-core path/to/esp32-2.0.17 \\
        --partitions ../esp32-ir-blaster/firmware

Only firmware/boot/ is written. publish_release.py never touches it: it only replaces
firmware/irblaster-*.bin and firmware/manifest.json.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
OUT = REPO / "firmware" / "boot"

# What firmware/platformio.ini builds for each published image: (manifest group, chip) -> details.
# flash_mode/flash_freq/boot_elf follow platform-espressif32 6.9.0 + arduino-esp32 2.0.17
# (builder/main.py _get_board_flash_mode/_get_board_boot_mode, tools/platformio-build.py).
TARGETS = {
    ("nodes", "esp32"): {
        "env": "esp32dev", "csv": "partitions.csv", "chip": "esp32",
        "boot_elf": "esp32/bin/bootloader_dio_40m.elf", "flash_mode": "dio", "flash_freq": "40m",
    },
    ("nodes", "esp32c3"): {
        "env": "esp32c3_supermini", "csv": "partitions.csv", "chip": "esp32c3",
        # esp32-c3-devkitm-1 is a qio board: the qio bootloader, with a dio header as PlatformIO writes it
        "boot_elf": "esp32c3/bin/bootloader_qio_80m.elf", "flash_mode": "dio", "flash_freq": "80m",
    },
    ("hubs", "esp32"): {
        "env": "esp32hub", "csv": "partitions_hub.csv", "chip": "esp32",
        "boot_elf": "esp32/bin/bootloader_dio_40m.elf", "flash_mode": "dio", "flash_freq": "40m",
    },
}
FLASH_SIZE = "4MB"
BOOTLOADER_OFFSET = {"esp32": 0x1000, "esp32c3": 0x0, "esp32s3": 0x0}
PARTITION_TABLE_OFFSET = 0x8000

# ---------------------------------------------------------------------------- partition tables
# The binary format of ESP-IDF's gen_esp32part.py: 32-byte entries, an MD5 entry, 0xFF up to 0xC00.

TYPES = {"app": 0x00, "data": 0x01}
SUBTYPES = {
    0x00: {"factory": 0x00, "test": 0x20, **{f"ota_{i}": 0x10 + i for i in range(16)}},
    0x01: {"ota": 0x00, "phy": 0x01, "nvs": 0x02, "coredump": 0x03, "nvs_keys": 0x04, "efuse": 0x05,
           "undefined": 0x06, "esphttpd": 0x80, "fat": 0x81, "spiffs": 0x82, "littlefs": 0x83},
}
TABLE_LEN = 0xC00


def _int(text: str) -> int:
    text = text.strip()
    for suffix, scale in (("k", 1024), ("m", 1024 * 1024)):
        if text.lower().endswith(suffix):
            return _int(text[:-1]) * scale
    return int(text, 0)


def partition_table(csv_path: Path) -> bytes:
    entries = b""
    for raw in csv_path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        fields = [f.strip() for f in line.split(",")]
        name, ptype, subtype, offset, size = fields[:5]
        flags = fields[5] if len(fields) > 5 else ""
        if flags:
            raise SystemExit(f"{csv_path}: flags ({flags}) are not supported here")
        if not offset or not size:
            raise SystemExit(f"{csv_path}: give every partition an explicit offset and size")
        type_id = TYPES[ptype] if ptype in TYPES else _int(ptype)
        sub_id = SUBTYPES.get(type_id, {}).get(subtype)
        if sub_id is None:
            sub_id = _int(subtype)
        if len(name.encode()) > 16:
            raise SystemExit(f"{csv_path}: partition name {name!r} is longer than 16 bytes")
        entries += struct.pack("<2sBBLL16sL", b"\xAA\x50", type_id, sub_id, _int(offset), _int(size), name.encode(), 0)
    table = entries + b"\xEB\xEB" + b"\xFF" * 14 + hashlib.md5(entries).digest()
    if len(table) >= TABLE_LEN:
        raise SystemExit(f"{csv_path}: partition table too long")
    return table + b"\xFF" * (TABLE_LEN - len(table))


def parse_table(data: bytes) -> list[dict]:
    parts = []
    for i in range(0, TABLE_LEN, 32):
        entry = data[i:i + 32]
        if entry[:2] != b"\xAA\x50":
            break
        _, ptype, sub, offset, size, label, _ = struct.unpack("<2sBBLL16sL", entry)
        parts.append({"type": ptype, "subtype": sub, "offset": offset, "size": size,
                      "name": label.rstrip(b"\0").decode()})
    return parts


def first_app_offset(table: bytes) -> int:
    for part in parse_table(table):
        if part["type"] == 0x00 and part["subtype"] in (0x00, 0x10):
            return part["offset"]
    raise SystemExit("partition table has no factory or ota_0 app partition")


def otadata_offset(table: bytes) -> int:
    for part in parse_table(table):
        if part["type"] == 0x01 and part["subtype"] == 0x00:
            return part["offset"]
    raise SystemExit("partition table has no otadata partition")

# ---------------------------------------------------------------------------- sources


def from_pio_build(build_dir: Path, boot_app0: Path) -> dict:
    files = {}
    for key, target in TARGETS.items():
        env_dir = build_dir / target["env"]
        bootloader = env_dir / "bootloader.bin"
        table = env_dir / "partitions.bin"
        for path in (bootloader, table):
            if not path.is_file():
                raise SystemExit(f"{path} is missing: run `pio run -e {target['env']}` first")
        files[key] = {"bootloader": bootloader.read_bytes(), "partitions": table.read_bytes(),
                      "otadata": boot_app0.read_bytes()}
    return files


def from_arduino_core(core: Path, csv_dir: Path) -> dict:
    sdk = core / "tools" / "sdk"
    boot_app0 = (core / "tools" / "partitions" / "boot_app0.bin").read_bytes()
    files = {}
    with tempfile.TemporaryDirectory() as tmp:
        for key, target in TARGETS.items():
            out = Path(tmp) / f"{key[0]}-{key[1]}.bin"
            subprocess.run(
                [sys.executable, "-m", "esptool", "--chip", target["chip"], "elf2image",
                 "--flash_mode", target["flash_mode"], "--flash_freq", target["flash_freq"],
                 "--flash_size", FLASH_SIZE, "-o", str(out), str(sdk / target["boot_elf"])],
                check=True, stdout=subprocess.DEVNULL)
            files[key] = {"bootloader": out.read_bytes(), "partitions": partition_table(csv_dir / target["csv"]),
                          "otadata": boot_app0}
    return files


def default_boot_app0() -> Path | None:
    for base in (Path.home() / ".platformio" / "packages",):
        candidate = base / "framework-arduinoespressif32" / "tools" / "partitions" / "boot_app0.bin"
        if candidate.is_file():
            return candidate
    return None

# ---------------------------------------------------------------------------- output


def write(files: dict, source: str) -> dict:
    if OUT.exists():
        shutil.rmtree(OUT)
    index: dict = {"schema": 1, "source": source}
    for (group, chip), parts in sorted(files.items()):
        product = "node" if group == "nodes" else "hub"
        folder = OUT / f"{product}-{chip}"
        folder.mkdir(parents=True)
        table = parts["partitions"]
        if len(table) != TABLE_LEN or table[:2] != b"\xAA\x50":
            raise SystemExit(f"{product}-{chip}: partitions.bin is not a partition table")
        if parts["bootloader"][:1] != b"\xE9":
            raise SystemExit(f"{product}-{chip}: bootloader.bin is not an ESP image")
        images = []
        layout = (
            ("bootloader", "bootloader.bin", BOOTLOADER_OFFSET[chip]),
            ("partitions", "partitions.bin", PARTITION_TABLE_OFFSET),
            ("otadata", "boot_app0.bin", otadata_offset(table)),
        )
        for name, filename, offset in layout:
            data = parts[name]
            (folder / filename).write_bytes(data)
            images.append({"name": name, "offset": offset, "file": f"{product}-{chip}/{filename}",
                           "size": len(data), "sha256": hashlib.sha256(data).hexdigest()})
        index.setdefault(group, {})[chip] = {
            "flash_size": FLASH_SIZE,
            "app_offset": first_app_offset(table),
            "images": images,
        }
    (OUT / "index.json").write_text(json.dumps(index, indent=2) + "\n", encoding="utf-8")
    return index


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--pio-build", type=Path, metavar="DIR", help="firmware/.pio/build of an esp32-ir-blaster checkout")
    source.add_argument("--arduino-core", type=Path, metavar="DIR", help="an extracted arduino-esp32 2.0.17 release")
    parser.add_argument("--boot-app0", type=Path, metavar="FILE",
                        help="boot_app0.bin (default: the one in ~/.platformio/packages/framework-arduinoespressif32)")
    parser.add_argument("--partitions", type=Path, metavar="DIR",
                        help="folder with partitions.csv and partitions_hub.csv (with --arduino-core)")
    args = parser.parse_args()

    if args.pio_build:
        boot_app0 = args.boot_app0 or default_boot_app0()
        if not boot_app0 or not boot_app0.is_file():
            parser.error("boot_app0.bin not found: pass --boot-app0")
        files = from_pio_build(args.pio_build, boot_app0)
        source = "firmware build (pio run)"
    else:
        if not args.partitions:
            parser.error("--arduino-core needs --partitions")
        files = from_arduino_core(args.arduino_core, args.partitions)
        source = "reconstructed from arduino-esp32 2.0.17 with esptool 4.5.1"
    index = write(files, source)
    print(json.dumps(index, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
