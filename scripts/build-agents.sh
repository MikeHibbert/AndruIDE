#!/bin/sh
# Builds andruide-agent for every platform AndruIDE can set up, into
# dist/agents/andruide-agent-<os>-<arch>. The app uploads the matching one
# to each machine (~/.andruide/bin) when it connects.
#
# On a Mac it also packs each macOS build as AndruIDE Agent.app, a hidden app
# with a reason for each permission a program in a session may ask for
# (agent/macos), into dist/agents/andruide-agent-darwin-<arch>.app.tar. That's
# what the app installs on Macs, in ~/.andruide/agent/<hash>.
#
# Needs: rustup targets (x86_64-apple-darwin, aarch64-apple-darwin,
# x86_64-unknown-linux-musl, aarch64-unknown-linux-musl, x86_64-pc-windows-gnu,
# aarch64-pc-windows-gnullvm), cargo-zigbuild and zig for the Linux builds
# (static musl binaries that run on any distro) and the Windows ones.
#
# ANDRUIDE_RELEASE=1 signs the macOS ones for notarization (hardened runtime and a
# secure timestamp), as scripts/bundle-macos.sh --release needs.
set -eu
cd "$(dirname "$0")/.."
out=dist/agents
mkdir -p "$out"

build() { # <target> <name> <cargo subcommand> [<.exe>]
  echo "▸ $2"
  cargo "$3" --release -p andruide-agent --target "$1" --quiet
  cp "target/$1/release/andruide-agent${4:-}" "$out/andruide-agent-$2${4:-}"
}

case "$(uname -s)" in
  Darwin)
    build aarch64-apple-darwin darwin-aarch64 build
    build x86_64-apple-darwin darwin-x86_64 build
    # Signed with a stable identity, a Mac keeps the agent's Screen Recording
    # and Accessibility permissions across updates (an ad-hoc signature
    # changes with every build, and the permissions go with it). By fingerprint:
    # a keychain can hold several certificates of the same name.
    identity=${ANDRUIDE_SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/^ *[0-9][0-9]*) \([0-9A-F]\{40\}\) "Developer ID Application: .*/\1/p' | head -n1)}
    if [ "${ANDRUIDE_RELEASE:-}" = 1 ]; then
      sign_flags="--options runtime --timestamp"
    else
      sign_flags="--timestamp=none"
    fi
    if [ -z "$identity" ] && [ "${ANDRUIDE_RELEASE:-}" = 1 ]; then
      echo "error: a release needs a Developer ID Application certificate in the keychain" >&2
      exit 1
    fi
    # AndruIDE Agent.app for each, signed with its entitlements, as a tar the app
    # streams to a Mac. Same identifier as the bare build, so the permissions
    # a Mac gave the agent (Screen Recording, Accessibility) carry over.
    version=$(sed -n 's/^version = "\(.*\)"/\1/p' agent/Cargo.toml | head -n1)
    for arch in aarch64 x86_64; do
      stage=$(mktemp -d)
      app="$stage/AndruIDE Agent.app"
      mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
      cp "$out/andruide-agent-darwin-$arch" "$app/Contents/MacOS/andruide-agent"
      sed "s/@VERSION@/$version/g" agent/macos/Info.plist > "$app/Contents/Info.plist"
      cp assets/brand/AndruIDE.icns "$app/Contents/Resources/AndruIDE.icns"
      # shellcheck disable=SC2086
      codesign --force $sign_flags --entitlements agent/macos/entitlements.plist -s "${identity:--}" "$app"
      codesign --verify --strict "$app"
      COPYFILE_DISABLE=1 tar --no-xattrs --no-mac-metadata -C "$stage" -cf "$out/andruide-agent-darwin-$arch.app.tar" "AndruIDE Agent.app"
      rm -rf "$stage"
    done
    if [ -n "$identity" ]; then
      for f in "$out"/andruide-agent-darwin-aarch64 "$out"/andruide-agent-darwin-x86_64; do
        # shellcheck disable=SC2086
        codesign --force $sign_flags --identifier com.lumarylabs.andruide-agent -s "$identity" "$f"
      done
      echo "▸ signed the macOS agents and AndruIDE Agent.app ($identity)"
    else
      echo "▸ no Developer ID certificate: the macOS agents are ad-hoc signed"
    fi
    ;;
esac
build x86_64-unknown-linux-musl linux-x86_64 zigbuild
build aarch64-unknown-linux-musl linux-aarch64 zigbuild
build x86_64-pc-windows-gnu windows-x86_64 zigbuild .exe
build aarch64-pc-windows-gnullvm windows-aarch64 zigbuild .exe
ls -lh "$out"
