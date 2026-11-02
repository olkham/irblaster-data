#!/usr/bin/env bash
# run.sh - start IR Blaster Recovery, building it first if needed (./install.sh).
#
#   ./run.sh                 open the window
#   ./run.sh --help          command-line options (--list, --info, --recover ...)
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"
if [ ! -x dist/irblaster-recovery ]; then
  ./install.sh
fi
exec dist/irblaster-recovery "$@"
