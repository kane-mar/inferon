#!/usr/bin/env bash
# Fetch the prompt-embedder artifact (ITEM-028 / ITEM-029).
#
#   scripts/fetch-embedder.sh [dest-dir]      # default: ~/.cache/inferon/embedder
#
# Downloads, once:
#   - TaylorAI/bge-micro-v2 quantized int8 ONNX (the 384-d model)
#   - its tokenizer.json
#   - ONNX Runtime 1.23.0 for this platform (the `ort` crate loads it dynamically)
#
# Idempotent: existing files are kept. Prints the env vars to export.
set -euo pipefail

dest="${1:-$HOME/.cache/inferon/embedder}"
model_url="https://huggingface.co/TaylorAI/bge-micro-v2/resolve/main/onnx/model_quantized.onnx"
tokenizer_url="https://huggingface.co/TaylorAI/bge-micro-v2/resolve/main/tokenizer.json"
# Newest ONNX Runtime that still publishes macOS x86_64 (and linux x64).
ort_version="1.19.2"

os=$(uname -s)
arch=$(uname -m)
case "$os-$arch" in
  Darwin-x86_64) ort_asset="onnxruntime-osx-x86_64-${ort_version}.tgz" ;;
  Darwin-arm64)  ort_asset="onnxruntime-osx-arm64-${ort_version}.tgz" ;;
  Linux-x86_64)  ort_asset="onnxruntime-linux-x64-${ort_version}.tgz" ;;
  Linux-aarch64) ort_asset="onnxruntime-linux-aarch64-${ort_version}.tgz" ;;
  *) echo "unsupported platform: $os-$arch" >&2; exit 1 ;;
esac

mkdir -p "$dest/ort"

fetch() { # url path
  if [ -s "$2" ]; then
    echo "have $(basename "$2")"
  else
    echo "fetching $(basename "$2")"
    curl -fsSL "$1" -o "$2.tmp"
    mv "$2.tmp" "$2"
  fi
}

fetch "$model_url" "$dest/model_quantized.onnx"
fetch "$tokenizer_url" "$dest/tokenizer.json"

# The runtime library, not the pkg-config file or headers.
lib_path() {
  find "$dest/ort/lib" -maxdepth 1 -type f \
    \( -name 'libonnxruntime*.dylib' -o -name 'libonnxruntime.so*' \) 2>/dev/null | head -1
}

if [ -z "$(lib_path)" ]; then
  echo "fetching $ort_asset"
  curl -fsSL \
    "https://github.com/microsoft/onnxruntime/releases/download/v${ort_version}/${ort_asset}" \
    -o "$dest/ort.tgz"
  # The release has one top-level dir; flatten it so `ort/` is the runtime root.
  rm -rf "$dest/ort" "$dest/ort-extract"
  mkdir -p "$dest/ort-extract"
  tar -xzf "$dest/ort.tgz" -C "$dest/ort-extract"
  inner=$(find "$dest/ort-extract" -mindepth 1 -maxdepth 1 -type d | head -1)
  mv "$inner" "$dest/ort"
  rm -rf "$dest/ort-extract" "$dest/ort.tgz"
fi

lib=$(lib_path)
if [ -z "$lib" ]; then
  echo "no libonnxruntime shared library under $dest/ort/lib" >&2
  exit 1
fi

cat <<EOF
export EMBED_MODEL_PATH="$dest/model_quantized.onnx"
export EMBED_TOKENIZER_PATH="$dest/tokenizer.json"
export ORT_DYLIB_PATH="$lib"
EOF
