IR Blaster Recovery for Linux

Reinstalls the firmware of an IR Blaster node or hub over USB.

Run it:
    ./irblaster-recovery            opens the window
    ./irblaster-recovery --help     the command-line mode

Serial port access: if the app says it isn't allowed to use the port, add yourself to the
serial-port group once, then log out and back in:
    sudo usermod -aG dialout $USER     (Debian, Ubuntu, Raspberry Pi OS)
    sudo usermod -aG uucp $USER        (Arch, Fedora)

Menu entry (optional): copy irblaster-recovery to ~/.local/bin, icon.png to
~/.local/share/icons/irblaster-recovery.png and irblaster-recovery.desktop to
~/.local/share/applications/.

Needs glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36 or later) and a desktop with X11
or Wayland for the window. The command-line mode needs neither.
