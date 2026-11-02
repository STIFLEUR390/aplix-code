#!/bin/sh
# Remove the `apc` binary installed by install.sh.
#
#   sh uninstall.sh            # the binary only; config, sessions and models stay on disk
#   sh uninstall.sh --purge    # also delete ~/.config/aplix and ~/.local/share/aplix
#
# Nothing else is touched: no PATH edit, no shell profile edit.
set -eu

INSTALL_DIR="${APC_INSTALL_DIR:-$HOME/.local/bin}"
binary="$INSTALL_DIR/apc"
config="${XDG_CONFIG_HOME:-$HOME/.config}/aplix"
data="${XDG_DATA_HOME:-$HOME/.local/share}/aplix"

purge=0
for arg in "$@"; do
  case "$arg" in
    --purge) purge=1 ;;
    -h | --help)
      sed -n '2,7p' "$0" | cut -c3-
      exit 0
      ;;
    *) echo "apc: unknown option '$arg' (try --help)" >&2 && exit 1 ;;
  esac
done

if [ -f "$binary" ]; then
  rm -f "$binary"
  echo "removed $binary"
else
  echo "no apc at $binary"
fi

if [ "$purge" = 1 ]; then
  rm -rf "$config" "$data"
  echo "removed $config and $data (config, sessions, models, serve token)"
else
  echo "kept $config and $data (config, sessions, models, serve token)"
  echo "re-run with --purge to delete them too"
fi
