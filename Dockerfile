# syntax=docker/dockerfile:1
# Builder pinned to -bookworm: the floating rust:1.98 tag drifted to a
# trixie base whose binaries need GLIBC_2.38 — pinned bookworm (glibc 2.36)
# matches the cc-debian12 runtime below.
FROM rust:1.98-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --features embedder
COPY . .
RUN cargo build --release --bin inferon --features embedder

# Prompt-embedder artifact (ITEM-028 AC3, ITEM-079): the quantized BGE-micro
# model, its tokenizer, and the ONNX Runtime `ort` loads dynamically. ~33 MB
# against the ~25 GB snapshot story. Pinned to ORT 1.19.2, the version whose
# `api-19` bindings the crate is compiled against.
#
# EMBEDDER_MIRROR lets a build fetch the same three artifacts from anywhere else
# — an air-gapped install (ITEM-083), or a day when GitHub's CDN answers 504 and
# the build has to get done. The files are pinned by version, so a mirror serves
# byte-identical bytes; `scripts/fetch-embedder.sh` lays one out in this shape.
ARG EMBEDDER_MIRROR=""
RUN set -eux; \
    mkdir -p /opt/embedder; \
    ort="https://github.com/microsoft/onnxruntime/releases/download/v1.19.2/onnxruntime-linux-x64-1.19.2.tgz"; \
    model="https://huggingface.co/TaylorAI/bge-micro-v2/resolve/main/onnx/model_quantized.onnx"; \
    tokenizer="https://huggingface.co/TaylorAI/bge-micro-v2/resolve/main/tokenizer.json"; \
    if [ -n "$EMBEDDER_MIRROR" ]; then \
        ort="$EMBEDDER_MIRROR/onnxruntime-linux-x64-1.19.2.tgz"; \
        model="$EMBEDDER_MIRROR/model_quantized.onnx"; \
        tokenizer="$EMBEDDER_MIRROR/tokenizer.json"; \
    fi; \
    curl -fsSL --retry 5 --retry-delay 3 --retry-all-errors "$ort" -o /tmp/ort.tgz; \
    tar -xzf /tmp/ort.tgz -C /tmp; \
    mv /tmp/onnxruntime-linux-x64-1.19.2 /opt/embedder/ort; \
    curl -fsSL --retry 5 --retry-delay 3 --retry-all-errors "$model" -o /opt/embedder/model.onnx; \
    curl -fsSL --retry 5 --retry-delay 3 --retry-all-errors "$tokenizer" -o /opt/embedder/tokenizer.json

FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
# The AGPL treats shipping an image as distribution, so the licence text travels
# with it (ITEM-111). The corresponding source is this repository at the release
# tag that matches the image.
COPY LICENSE /LICENSE
COPY --from=builder /app/target/release/inferon /usr/local/bin/inferon
COPY --from=builder /opt/embedder /opt/embedder
# The embedder loads the runtime itself (no link at build time).
ENV EMBED_MODEL_PATH=/opt/embedder/model.onnx \
    EMBED_TOKENIZER_PATH=/opt/embedder/tokenizer.json \
    ORT_DYLIB_PATH=/opt/embedder/ort/lib/libonnxruntime.so.1.19.2
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/inferon"]
