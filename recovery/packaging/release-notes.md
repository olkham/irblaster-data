Reinstalls the firmware of an IR Blaster node or hub over USB, for a board that won't start or can't be updated from the hub.

| System | Download |
|---|---|
| Windows 10 and 11 (64-bit) | `irblaster-recovery-windows-x64.exe` |
| macOS 11 or later (Apple silicon and Intel) | `irblaster-recovery-macos.zip` |
| Linux (x86-64) | `irblaster-recovery-linux-x64.tar.gz` |
| Linux (ARM64, e.g. Raspberry Pi OS 64-bit) | `irblaster-recovery-linux-arm64.tar.gz` |

Open it, plug the board in, click **Connect**, then **Recover**. It downloads the current firmware from the IR Blaster site.

**First launch:** the app isn't code-signed yet. On Windows choose *More info → Run anyway*. On macOS, open it once, then go to *System Settings → Privacy & Security* and choose *Open Anyway*.

`SHA256SUMS.txt` lists the checksum of every file.
