FROM rust:1-alpine AS chef
RUN apk add --no-cache musl-dev
RUN cargo install cargo-chef
WORKDIR /app

# Analyze dependencies and produce a recipe file
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
RUN cargo chef prepare --recipe-path recipe.json

# Build dependencies first (cached), then build the app
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
RUN cargo build --release

# Minimal runtime image with just the binary and static assets
FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=builder /app/target/release/sdict /usr/local/bin/sdict
COPY static /app/static
WORKDIR /app
ENV PORT=3000
EXPOSE 3000
CMD ["sdict"]
