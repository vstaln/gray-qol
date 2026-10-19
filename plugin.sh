#!/bin/sh
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
sys="$(uname -s)"
case "$sys" in
MINGW* | MSYS* | CYGWIN*) rel="x86_64-pc-windows-msvc/qol.exe" ;;
Darwin*)
    case "$(uname -m)" in
    arm64) rel="aarch64-apple-darwin/qol" ;;
    *) rel="x86_64-apple-darwin/qol" ;;
    esac ;;
*)
    case "$(uname -m)" in
    aarch64 | arm64) rel="aarch64-unknown-linux-musl/qol" ;;
    *) rel="x86_64-unknown-linux-musl/qol" ;;
    esac ;;
esac
if [ -x "$here/$rel" ]; then exec "$here/$rel" "$@"; fi
for cand in "$here/target/release/qol.exe" "$here/target/release/qol" "$here/target/debug/qol.exe" "$here/target/debug/qol"; do
    if [ -x "$cand" ]; then exec "$cand" "$@"; fi
done
echo "qol binary not found in $here" >&2
exit 1
