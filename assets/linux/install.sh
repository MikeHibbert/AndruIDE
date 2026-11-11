#!/bin/sh
# Installs AndruIDE for you (no root needed): the app in ~/.local/share/andruide-app,
# the `andruide` command in ~/.local/bin (and `andruide`, its name from before), and
# AndruIDE in your desktop's app menu. Run it again to update; it replaces an
# install from when it was called AndruIDE. `./install.sh --uninstall` takes it
# all out again (your settings in ~/.config/andruide stay).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
dest=$data/andruide-app
bin=$HOME/.local/bin
entry=$data/applications/andruide.desktop
icon=$data/icons/hicolor/512x512/apps/andruide.png

# From before the rename: the app's folder, its menu entry and icon, and the
# andruide command when it ran that copy.
old=$data/andruide-app
[ "$(readlink "$bin/andruide" 2>/dev/null)" = "$old/andruide" ] && rm -f "$bin/andruide"
rm -rf "$old" "$data/applications/andruide.desktop" "$data/icons/hicolor/512x512/apps/andruide.png"

if [ "${1:-}" = --uninstall ]; then
    for name in andruide andruide; do
        [ "$(readlink "$bin/$name" 2>/dev/null)" = "$dest/andruide" ] && rm -f "$bin/$name"
    done
    rm -rf "$dest" "$entry" "$icon"
    echo "AndruIDE is uninstalled. Your settings are still in ~/.config/andruide."
    exit 0
fi

rm -rf "$dest"
mkdir -p "$dest" "$bin" "$(dirname "$entry")" "$(dirname "$icon")"
cp -R "$here/andruide" "$here/agents" "$dest/"
ln -sf "$dest/andruide" "$bin/andruide"
# andruide too, unless something else has that name.
case "$(readlink "$bin/andruide" 2>/dev/null)" in
    */andruide | */andruide) ln -sf "$dest/andruide" "$bin/andruide" ;;
    "") [ -e "$bin/andruide" ] || ln -s "$dest/andruide" "$bin/andruide" ;;
esac
cp "$here/andruide.png" "$icon"
sed "s|^Exec=.*|Exec=$dest/andruide|" "$here/andruide.desktop" > "$entry"
command -v update-desktop-database >/dev/null && update-desktop-database "$(dirname "$entry")" 2>/dev/null || true

echo "AndruIDE is installed: open it from your app menu, or run \`andruide\`."
case ":$PATH:" in
    *":$bin:"*) ;;
    *) echo "To use \`andruide\` in a terminal, add $bin to your PATH." ;;
esac
