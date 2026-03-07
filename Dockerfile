FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
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
