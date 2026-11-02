# IR Blaster Recovery

A small desktop app that reinstalls the firmware of an IR Blaster node or hub over USB: for a board
that won't start, was flashed with something else, or can't be updated from the hub. One executable
per system, nothing to install:

| System | File |
|---|---|
| Windows 10 and 11 (64-bit) | `irblaster-recovery-windows-x64.exe` |
| macOS 11 or later (Apple silicon and Intel) | `irblaster-recovery-macos.zip` (an app) |
| Linux x86-64 / ARM64 | `irblaster-recovery-linux-x64.tar.gz` / `-linux-arm64.tar.gz` |

Downloads are on the [releases page](https://github.com/olkham/irblaster-data/releases) and on the
[IR Blaster site](https://olkham.github.io/irblaster-data/#recovery).

## Using it

1. Plug the board in with a USB data cable and open the app.
2. **Connect**: the app finds the board, shows its chip and what it runs now.
3. Choose **IR Blaster node** or **IR Hub** (the app preselects what the board has), or use a firmware
   file from this computer (drag it onto the window).
4. **Recover**. It takes about a minute; the board restarts with the new firmware.

Settings, WiFi details and learned buttons are kept when the board already has the same storage layout.
Tick **Erase everything first** to wipe them, for a board that still doesn't start after a normal
recovery. Changing a board between node and hub always erases it, because the layouts differ.

If it can't connect: use a data cable (some only charge), and hold the board's **BOOT** button while
plugging it in, or hold BOOT and press RST/EN once.

### Command line

The same app runs without a window when given arguments, for support and scripting:

```bash
irblaster-recovery --list                                  # serial ports; * marks likely boards
irblaster-recovery --info                                  # the connected board and what it runs
irblaster-recovery --recover                               # reinstall what it has, asks first
irblaster-recovery --recover --product hub --erase --yes   # turn it into a hub, no questions
irblaster-recovery --recover --image irblaster-esp32-26.11.01.2146.bin   # offline, from a file
irblaster-recovery --help
```

`--no-reset` connects to a board that is already in download mode (BOOT held while it powered up), for
boards without the auto-reset circuit.

### First launch on each system

* **Windows**: SmartScreen warns about an unknown publisher until the app is code-signed: choose
  *More info → Run anyway*. Boards with a CP210x or CH340 USB chip need its driver, which Windows
  Update normally installs within a minute of plugging the board in.
* **macOS**: the app is not notarised yet. Open it once, then go to *System Settings → Privacy &
  Security* and choose *Open Anyway*.
* **Linux**: add yourself to the serial-port group once (`sudo usermod -aG dialout $USER`, `uucp` on
  Arch and Fedora) and log in again. Needs glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36 or
  later). The window needs X11 or Wayland; the command line needs neither.

## What it writes

The firmware images in `firmware/` are over-the-air updates: on their own they can't bring back a
board whose bootloader or partition table is gone. A recovery writes the full set:

| Offset | What | From |
|---|---|---|
| `0x1000` (ESP32) or `0x0` (ESP32-C3/S3) | second-stage bootloader | `firmware/boot/<product>-<chip>/bootloader.bin` |
| `0x8000` | partition table | `…/partitions.bin` |
| `0xe000` | OTA data: boot the first app slot | `…/boot_app0.bin` |
| `0x10000` | the firmware | `firmware/manifest.json` → `firmware/<file>` |

Before touching the board it downloads everything and checks it: each file against the SHA-256
published next to it, the firmware's chip against the connected chip, its size against the app slot,
and its signature (see *Trusted keys*). It reads the board's partition table first, to tell what it
runs and whether a full erase is needed. After writing, each region is verified against an MD5 the
chip computes from its flash.

`firmware/boot/` is built into the app as well, so recovery from a local firmware file works offline.

## Building from source

```bash
./install.sh          # Linux/macOS: checks for Rust (offers to install it), builds into dist/
./run.sh              # starts the app; ./run.sh --help for the command line
./install.sh --clean  # delete earlier builds and build from scratch
```

On Windows use `install.bat` and `run.bat`; they also offer to install the Visual Studio C++ build
tools, which Rust needs on Windows.

Without installing Rust, build the Linux app with Docker:

```bash
docker compose run --rm build      # writes dist/irblaster-recovery
```

`docker compose --profile cli run --rm cli --recover` runs the command line in the container, with the
port from `IRBLASTER_PORT` passed in (Linux hosts only).

Development: `cargo test` (unit tests, including the download path against a local HTTP server),
`cargo clippy --all-targets`, `cargo fmt`. `IRBLASTER_RECOVERY_URL` points the window at another copy
of the site (the command line uses `--base-url`); `IRBLASTER_RECOVERY_THEME=light|dark` overrides the
system theme.

## Releasing

CI (`.github/workflows/recovery.yml`) lints, tests and builds every change for all four targets; the
builds are attached to each run. To publish a release:

1. Bump `version` in `Cargo.toml` and merge.
2. Tag the merge commit `recovery-v<version>` and push the tag:

   ```bash
   git tag recovery-v0.1.0 && git push origin recovery-v0.1.0
   ```

The workflow then creates a GitHub Release with the four builds and `SHA256SUMS.txt`, and the landing
page picks it up by itself.

## Keeping it in step with the firmware

* **Boot set.** `firmware/boot/` must match how the firmware is built. After changing a partition
  table, the board list or the Arduino core in the firmware repository, refresh it from a build:

  ```bash
  python recovery/scripts/make_boot.py --pio-build ../esp32-ir-blaster/firmware/.pio/build
  ```

  The current set was reconstructed from the arduino-esp32 2.0.17 release (bootloader images built with
  esptool 4.5.1 and the board settings espressif32 @ 6.9.0 uses). Partition tables are byte-identical to
  ESP-IDF's generator, and the bootloader headers match the published firmware (DIO, 4 MB, 40/80 MHz).
  Replacing them with real build outputs as above is still worthwhile. Ideally `publish_release.py`
  does it on every publish.
* **Trusted keys.** `trusted_keys.txt` lists the Ed25519 keys allowed to sign firmware, the same as
  `firmware/src/update_key.h`. It is empty for now, so signatures aren't checked yet (downloads are
  still checked against their published SHA-256). Add the public key once the firmware is armed;
  from then on the app refuses published images without a valid signature for the chosen product and
  chip, and asks before flashing an unsigned local file.
* **Icons.** `python recovery/scripts/make_icons.py` redraws `assets/` from the brand mark.

## Status

Not yet run against real hardware: the flashing goes through [espflash](https://github.com/esp-rs/espflash)
(the same library as the `espflash` tool), and the boot set, layout and checks are covered by unit tests,
but test a recovery of each board type (ESP32 node, ESP32-C3 node, ESP32 hub) before relying on it.
Code signing (Windows Authenticode, Apple notarisation) is not set up yet.
