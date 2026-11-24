#!/usr/bin/env bash
# Build the standalone JNI API v1 asset from this checkout.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd -P)
target_dir=${CARGO_TARGET_DIR:-"$root/android/target"}
output=${1:-"$target_dir/dist"}
# Keep caller-relative output and Cargo paths meaningful when invoking cargo.
mkdir -p "$output" "$target_dir"
output=$(cd "$output" && pwd -P)
export CARGO_TARGET_DIR=$(cd "$target_dir" && pwd -P)

case "$(uname -s)" in
    Darwin) host=darwin-x86_64 ;;
    Linux) host=linux-x86_64 ;;
    *) echo "Use a macOS or Linux NDK host." >&2; exit 1 ;;
esac
bin="${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to the Android NDK}/toolchains/llvm/prebuilt/$host/bin"
export CC_aarch64_linux_android="$bin/aarch64-linux-android26-clang"
export AR_aarch64_linux_android="$bin/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CC_aarch64_linux_android"

cd "$root/android"
cargo build --lib --locked --release --target aarch64-linux-android
library="$CARGO_TARGET_DIR/aarch64-linux-android/release/libpikpak_android.so"
export LC_ALL=C
"$bin/llvm-readelf" -h "$library" | awk '
    /Class:/ && /ELF64/ { class = 1 }
    /Type:/ && /DYN/ { dyn = 1 }
    /Machine:/ && /AArch64/ { arm = 1 }
    END { exit !(class && dyn && arm) }
'
"$bin/llvm-readelf" -lW "$library" | awk '
    $1 == "LOAD" { count++; if ($NF != "0x4000") bad = 1 }
    END { exit (!count || bad) }
'
"$bin/llvm-nm" -D --defined-only "$library" | awk '
    $2 == "T" && $3 == "Java_com_snaix_pikpak_NativeClient_request" { found = 1 }
    END { exit !found }
'
asset="$output/pikpaktui-android-arm64-v8a-api1.so"
install -m 0644 "$library" "$asset"
echo "$asset"
