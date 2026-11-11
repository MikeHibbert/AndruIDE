#!/bin/sh
# Brings AndruIDE's AUR package (andruide-bin: `yay -S andruide-bin`) up to a
# published release: its PKGBUILD and .SRCINFO, with the release's version and
# the Linux tarballs' checksums from its SHA256SUMS.txt. It installs the app
# like the .pkg.tar.zst on the release does: /usr/lib/andruide, `andruide` and
# `andruide` in /usr/bin, the app menu entry and icon.
#
#   scripts/aur.sh 0.5.2           # write it in a clone of the AUR repo and commit
#   scripts/aur.sh 0.5.2 --push    # and push it (your AUR ssh key)
#
# AUR_DIR=<folder> uses a clone you already have. ANDRUIDE_SUMS_FILE replaces
# the release's checksums (a local test). See RELEASING.md.
set -eu

cd "$(dirname "$0")/.."
usage="usage: scripts/aur.sh <version> [--push]"
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
x86_64=$(sha "AndruIDE-$version-linux-x86_64.tar.gz")
aarch64=$(sha "AndruIDE-$version-linux-aarch64.tar.gz")
[ -n "$x86_64" ] && [ -n "$aarch64" ] || {
    echo "error: v$version's SHA256SUMS.txt doesn't have the Linux tarballs" >&2
    exit 1
}
release=https://github.com/MikeHibbert/AndruIDE/releases/download/v$version
desc="All your machines in one app, for you and your AI agents"
depends="glibc libxcb libxkbcommon libxkbcommon-x11 vulkan-icd-loader"

aur=${AUR_DIR:-}
if [ -z "$aur" ]; then
    aur=$(mktemp -d)
    # An empty repository the first time: pushing to it creates the package.
    git clone --quiet ssh://aur@aur.archlinux.org/andruide-bin.git "$aur" 2>/dev/null || git -C "$aur" init --quiet -b master
    git -C "$aur" remote get-url origin >/dev/null 2>&1 || git -C "$aur" remote add origin ssh://aur@aur.archlinux.org/andruide-bin.git
fi

cat > "$aur/PKGBUILD" <<'EOF'
# Maintainer: Adib Hanna <adibhanna@gmail.com>
#
# AndruIDE (yay -S andruide-bin): written by scripts/aur.sh in
# https://github.com/MikeHibbert/AndruIDE from each release.

pkgname=andruide-bin
pkgver=@VERSION@
pkgrel=1
pkgdesc="@DESC@"
arch=('x86_64' 'aarch64')
url="https://muxin.dev"
license=('Apache-2.0')
depends=(@DEPENDS@)
optdepends=('wayland: to run in a Wayland session'
            'vulkan-driver: to draw its window (Mesa or your GPU maker'"'"'s)')
provides=('andruide')
conflicts=('andruide')
options=('!strip' '!debug')
source_x86_64=("AndruIDE-${pkgver}-linux-x86_64.tar.gz::https://github.com/MikeHibbert/AndruIDE/releases/download/v${pkgver}/AndruIDE-${pkgver}-linux-x86_64.tar.gz")
source_aarch64=("AndruIDE-${pkgver}-linux-aarch64.tar.gz::https://github.com/MikeHibbert/AndruIDE/releases/download/v${pkgver}/AndruIDE-${pkgver}-linux-aarch64.tar.gz")
sha256sums_x86_64=('@SHA_X86_64@')
sha256sums_aarch64=('@SHA_AARCH64@')

package() {
  cd "${srcdir}/AndruIDE"

  # The app, beside the agent builds it sets up on your machines.
  install -Dm755 andruide "${pkgdir}/usr/lib/andruide/andruide"
  install -dm755 "${pkgdir}/usr/lib/andruide/agents"
  install -m755 agents/* "${pkgdir}/usr/lib/andruide/agents/"

  # The command, and its name from before (AndruIDE).
  install -dm755 "${pkgdir}/usr/bin"
  ln -s /usr/lib/andruide/andruide "${pkgdir}/usr/bin/andruide"
  ln -s /usr/lib/andruide/andruide "${pkgdir}/usr/bin/andruide"

  install -Dm644 andruide.desktop "${pkgdir}/usr/share/applications/andruide.desktop"
  install -Dm644 andruide.png "${pkgdir}/usr/share/icons/hicolor/512x512/apps/andruide.png"
  install -Dm644 LICENSE NOTICE THIRD_PARTY_NOTICES.md -t "${pkgdir}/usr/share/licenses/${pkgname}/"
}
EOF
quoted=$(for d in $depends; do printf "'%s' " "$d"; done | sed 's/ $//')
sed -i.bak -e "s|@VERSION@|$version|" -e "s|@DESC@|$desc|" -e "s|@DEPENDS@|$quoted|" \
    -e "s|@SHA_X86_64@|$x86_64|" -e "s|@SHA_AARCH64@|$aarch64|" "$aur/PKGBUILD"
rm -f "$aur/PKGBUILD.bak"

# What `makepkg --printsrcinfo` writes for that PKGBUILD (no Arch tools needed).
{
    printf 'pkgbase = andruide-bin\n'
    printf '\tpkgdesc = %s\n\tpkgver = %s\n\tpkgrel = 1\n\turl = https://muxin.dev\n' "$desc" "$version"
    printf '\tarch = x86_64\n\tarch = aarch64\n\tlicense = Apache-2.0\n'
    for d in $depends; do printf '\tdepends = %s\n' "$d"; done
    printf '\toptdepends = wayland: to run in a Wayland session\n'
    printf "\toptdepends = vulkan-driver: to draw its window (Mesa or your GPU maker's)\n"
    printf '\tprovides = andruide\n\tconflicts = andruide\n\toptions = !strip\n\toptions = !debug\n'
    for a in x86_64 aarch64; do
        eval "sum=\$$a"
        printf '\tsource_%s = AndruIDE-%s-linux-%s.tar.gz::%s/AndruIDE-%s-linux-%s.tar.gz\n' "$a" "$version" "$a" "$release" "$version" "$a"
        printf '\tsha256sums_%s = %s\n' "$a" "$sum"
    done
    printf '\npkgname = andruide-bin\n'
} > "$aur/.SRCINFO"
echo "▸ $aur: andruide-bin $version-1"

cd "$aur"
git add PKGBUILD .SRCINFO
if git diff --cached --quiet; then
    echo "▸ already at $version"
    exit 0
fi
git commit --quiet -m "andruide-bin $version-1"
if [ -n "$push" ]; then
    git push --quiet origin HEAD:master
    echo "▸ pushed: yay -S andruide-bin gets it"
else
    echo "Push it from $aur: git push origin HEAD:master"
fi
