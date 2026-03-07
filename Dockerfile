FROM rust:1-alpine AS chef
RUN apk add --no-cache musl-dev
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
RUN cargo build --release

FROM alpine:3
RUN apk add --no-cache ca-certificates && \
    addgroup -S app && adduser -S app -G app
COPY --from=builder /app/target/release/sdict /usr/local/bin/sdict
COPY static /app/static
WORKDIR /app
USER app
ENV PORT=3000
EXPOSE 3000
CMD ["sdict"]
