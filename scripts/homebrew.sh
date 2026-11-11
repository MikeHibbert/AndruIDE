#!/bin/sh
# Brings AndruIDE's Homebrew cask up to a published release: Casks/andruide.rb in
# LumaryLabsLLC/homebrew-tap, with the release's version and the checksums
# from its SHA256SUMS.txt. One cask for macOS (the app, in Applications) and
# Linux (the app in Homebrew's folder, with an entry in the app menu); both
# put `andruide` (and `andruide`, its name from before) in the PATH.
#
#   scripts/homebrew.sh 0.5.1           # write it in a clone of the tap and commit
#   scripts/homebrew.sh 0.5.1 --push    # and push it
#
# HOMEBREW_TAP_DIR=<folder> uses a clone you already have. For a local test,
# ANDRUIDE_CASK_URL replaces the download folder and ANDRUIDE_SUMS_FILE the
# checksums. See RELEASING.md.
set -eu

cd "$(dirname "$0")/.."
usage="usage: scripts/homebrew.sh <version> [--push]"
version=${1:?$usage}
version=${version#v}
push=
case "${2:-}" in
    --push) push=1 ;;
    "") ;;
    *) echo "$usage" >&2; exit 2 ;;
esac

sums=$(if [ -n "${ANDRUIDE_SUMS_FILE:-}" ]; then cat "$ANDRUIDE_SUMS_FILE"; else gh release download "v$version" -p SHA256SUMS.txt -O -; fi)
sha() { echo "$sums" | awk -v f="$1" '$2 == f || $2 == "*" f { print $1 }'; }
macos=$(sha "AndruIDE-$version-macos.dmg")
x86_64=$(sha "AndruIDE-$version-linux-x86_64.tar.gz")
aarch64=$(sha "AndruIDE-$version-linux-aarch64.tar.gz")
[ -n "$macos" ] && [ -n "$x86_64" ] && [ -n "$aarch64" ] || {
    echo "error: v$version's SHA256SUMS.txt doesn't have the macOS and Linux files" >&2
    exit 1
}
base=${ANDRUIDE_CASK_URL:-'https://github.com/MikeHibbert/AndruIDE/releases/download/v#{version}'}

tap=${HOMEBREW_TAP_DIR:-}
if [ -z "$tap" ]; then
    tap=$(mktemp -d)
    gh repo clone LumaryLabsLLC/homebrew-tap "$tap" -- --quiet
fi
mkdir -p "$tap/Casks"
cat > "$tap/Casks/andruide.rb" <<EOF
cask "andruide" do
  arch arm: "aarch64", intel: "x86_64"

  version "$version"
  sha256 arm:          "$macos",
         intel:        "$macos",
         arm64_linux:  "$aarch64",
         x86_64_linux: "$x86_64"

  on_macos do
    url "$base/AndruIDE-#{version}-macos.dmg"

    depends_on macos: :monterey

    app "AndruIDE.app"
    binary "#{appdir}/AndruIDE.app/Contents/MacOS/andruide"
    binary "#{appdir}/AndruIDE.app/Contents/MacOS/andruide", target: "andruide"
  end
  on_linux do
    url "$base/AndruIDE-#{version}-linux-#{arch}.tar.gz"

    binary "AndruIDE/andruide"
    binary "AndruIDE/andruide", target: "andruide"
    artifact "AndruIDE/andruide.desktop", target: "~/.local/share/applications/andruide.desktop"

    preflight_steps do
      inreplace "AndruIDE/andruide.desktop", "Exec=andruide", "Exec={{HOMEBREW_PREFIX}}/bin/andruide"
      inreplace "AndruIDE/andruide.desktop", "Icon=andruide", "Icon={{staged_path}}/AndruIDE/andruide.png"
    end
  end

  name "AndruIDE"
  desc "All your machines in one app, for you and your AI agents"
  homepage "https://muxin.dev/"

  livecheck do
    url :url
    strategy :github_latest
  end

  zap trash: [
    "~/.cache/andruide",
    "~/.config/andruide",
    "~/.config/andruide",
    "~/.andruide",
    "~/Library/Caches/andruide",
    "~/Library/Preferences/com.lumarylabs.andruide.plist",
  ]
end
EOF
echo "▸ $tap/Casks/andruide.rb: $version"

cd "$tap"
if git diff --quiet -- Casks/andruide.rb && git ls-files --error-unmatch Casks/andruide.rb >/dev/null 2>&1; then
    echo "▸ already at $version"
    exit 0
fi
git add Casks/andruide.rb
git commit --quiet -m "andruide $version"
if [ -n "$push" ]; then
    git push --quiet origin HEAD
    echo "▸ pushed: brew upgrade --cask andruide gets it"
else
    echo "Push it from $tap: git push origin HEAD"
fi
