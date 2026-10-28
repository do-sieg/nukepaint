#!/bin/sh
# Builds NukePaint and installs it for the current user: the binary in
# ~/.cargo/bin, plus a menu entry and icon so it opens images from the file
# manager. Run it again to update. Pass --uninstall to remove everything.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
apps=$data/applications
icons=$data/icons/hicolor
sizes="16 24 32 48 64 128 256 512"

# Earlier versions installed an SVG icon, which would take precedence.
rm -f "$icons/scalable/apps/nukepaint.svg"

if [ "${1:-}" = "--uninstall" ]; then
    cargo uninstall nukepaint || true
    rm -f "$apps/nukepaint.desktop"
    for size in $sizes; do
        rm -f "$icons/${size}x$size/apps/nukepaint.png"
    done
else
    cargo install --locked --path "$root/app"
    bin=$(command -v nukepaint || echo "${CARGO_HOME:-$HOME/.cargo}/bin/nukepaint")

    # An absolute path, as the menu may not see the shell's PATH.
    mkdir -p "$apps"
    sed "s|^Exec=nukepaint|Exec=$bin|" "$root/packaging/linux/nukepaint.desktop" > "$apps/nukepaint.desktop"
    for size in $sizes; do
        mkdir -p "$icons/${size}x$size/apps"
        if [ "$size" = 512 ]; then
            cp "$root/icon.png" "$icons/${size}x$size/apps/nukepaint.png"
        else
            cp "$root/packaging/linux/icons/$size.png" "$icons/${size}x$size/apps/nukepaint.png"
        fi
    done
fi

update-desktop-database "$apps" 2>/dev/null || true
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
echo "Done."
