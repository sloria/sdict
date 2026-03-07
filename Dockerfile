FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
RUN cargo build --release

FROM alpine:3
RUN apk add --no-cache ca-certificates
COPY --from=builder /app/target/release/sdict /usr/local/bin/sdict
ENV PORT=3000
EXPOSE 3000
CMD ["sdict"]
