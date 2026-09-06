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
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin inferon

FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
COPY --from=builder /app/target/release/inferon /usr/local/bin/inferon
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/inferon"]
