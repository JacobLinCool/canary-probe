FROM rust:alpine as builder

WORKDIR /usr/src/app
RUN apk update && apk add --no-cache musl-dev
COPY canary-probe/Cargo.toml ./
RUN mkdir src && \
    echo "fn main() {println!(\"if you see this, the build broke\")}" > src/main.rs && \
    cargo build --release && \
    rm -rf src
COPY canary-probe/src ./src
RUN cargo build --release

FROM alpine:latest

WORKDIR /x
COPY --from=builder /usr/src/app/target/release/canary-probe /usr/local/bin/canary-probe

ENTRYPOINT ["canary-probe"]
