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
RUN set -eux; \
    mkdir -p /opt/embedder; \
    curl -fsSL "https://github.com/microsoft/onnxruntime/releases/download/v1.19.2/onnxruntime-linux-x64-1.19.2.tgz" -o /tmp/ort.tgz; \
    tar -xzf /tmp/ort.tgz -C /tmp; \
    mv /tmp/onnxruntime-linux-x64-1.19.2 /opt/embedder/ort; \
    curl -fsSL "https://huggingface.co/TaylorAI/bge-micro-v2/resolve/main/onnx/model_quantized.onnx" -o /opt/embedder/model.onnx; \
    curl -fsSL "https://huggingface.co/TaylorAI/bge-micro-v2/resolve/main/tokenizer.json" -o /opt/embedder/tokenizer.json

FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
COPY --from=builder /app/target/release/inferon /usr/local/bin/inferon
COPY --from=builder /opt/embedder /opt/embedder
# The embedder loads the runtime itself (no link at build time).
ENV EMBED_MODEL_PATH=/opt/embedder/model.onnx \
    EMBED_TOKENIZER_PATH=/opt/embedder/tokenizer.json \
    ORT_DYLIB_PATH=/opt/embedder/ort/lib/libonnxruntime.so.1.19.2
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/inferon"]
