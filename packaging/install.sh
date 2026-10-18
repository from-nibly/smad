#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
prefix=${PREFIX:-/usr/local}
destdir=${DESTDIR:-}
profile=release
build=1

usage() {
  cat <<'EOF'
Usage: packaging/install.sh [--prefix PATH] [--destdir PATH] [--profile debug|release] [--no-build]

Installs SMAD's native binaries, desktop metadata, and optional
Zellij plugin. DESTDIR is honored for package staging; installed desktop and
plugin paths continue to use PREFIX.
EOF
}

while (($#)); do
  case $1 in
    --prefix) prefix=$2; shift 2 ;;
    --destdir) destdir=$2; shift 2 ;;
    --profile) profile=$2; shift 2 ;;
    --no-build) build=0; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "install.sh: unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

case $profile in
  debug) cargo_profile=debug; cargo_flags=() ;;
  release) cargo_profile=release; cargo_flags=(--release) ;;
  *) echo "install.sh: profile must be debug or release" >&2; exit 2 ;;
esac

if ((build)); then
  cargo build --locked "${cargo_flags[@]}" -p smad -p smad-cli
  cargo build --locked "${cargo_flags[@]}" -p smad-zellij --target wasm32-wasip1
fi

native_dir=${CARGO_TARGET_DIR:-$root/target}/$cargo_profile
wasm_dir=${CARGO_TARGET_DIR:-$root/target}/wasm32-wasip1/$cargo_profile
stage=${destdir%/}${prefix}

install -d "$stage/bin" "$stage/share/applications" "$stage/share/icons/hicolor/scalable/apps" \
  "$stage/share/smad/zellij"
install -m 0755 "$native_dir/smad" "$stage/bin/smad"
install -m 0755 "$native_dir/smad-axi" "$stage/bin/smad-axi"
install -m 0755 "$native_dir/lavish-open" "$stage/bin/lavish-open"
install -m 0755 "$native_dir/lavish-md-open" "$stage/bin/lavish-md-open"
install -m 0755 "$native_dir/smad-ctl" "$stage/bin/smad-ctl"
install -m 0755 "$native_dir/smad-herdr-sync" "$stage/bin/smad-herdr-sync"
install -m 0644 "$wasm_dir/smad_zellij.wasm" \
  "$stage/share/smad/zellij/smad-zellij.wasm"
install -m 0644 "$root/assets/works.from-nibly.Smad.desktop" "$stage/share/applications/"
install -m 0644 "$root/assets/works.from-nibly.Smad.Uri.desktop" "$stage/share/applications/"
install -d "$stage/share/smad/docs"
install -m 0644 "$root/README.md" "$stage/share/smad/README.md"
cp -R "$root/docs/." "$stage/share/smad/docs/"
install -d "$stage/share/smad/skills"
cp -R "$root/skills/smad" "$stage/share/smad/skills/"
install -d "$stage/share/smad/licenses"
cp -R "$root/licenses/." "$stage/share/smad/licenses/"
python3 "$root/packaging/licenses.py" "$stage/share/smad/licenses/cargo"
# Transition entry points; new integrations should use smad-axi.
ln -sfn smad "$stage/bin/lavish-browser"
ln -sfn smad "$stage/bin/smad-server"
ln -sfn smad-ctl "$stage/bin/lavish-browser-ctl"
ln -sfn smad-herdr-sync "$stage/bin/lavish-browser-herdr-sync"
install -m 0644 "$root/assets/works.from-nibly.Smad.svg" \
  "$stage/share/icons/hicolor/scalable/apps/works.from-nibly.Smad.svg"

printf 'Installed SMAD under %s\n' "$stage"
printf 'HerdR lifecycle helper: %s/bin/smad-herdr-sync\n' "$prefix"
printf 'Optional Zellij plugin: %s/share/smad/zellij/smad-zellij.wasm\n' "$prefix"
