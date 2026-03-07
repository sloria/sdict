# sdict

A self-hostable ad-free web frontend for [SpanishDict](https://www.spanishdict.com/).

## Run with Docker

```bash
docker build -t sdict .
docker run -p 3000:3000 sdict
```

Or with Docker Compose:

```yaml
services:
  sdict:
    build: .
    ports:
      - "3000:3000"
```

```bash
docker compose up
```

## Environment variables

- `PORT` — Port to listen on (default: `3000`)
- `RUST_LOG` — Log level filter (default: `info`)
- `SENTRY_DSN` — Sentry or GlitchTip DSN. If unset, error tracking is disabled
- `SENTRY_ENV` — Sentry environment, e.g. `production`, `staging`

## Development

```bash
# Run the dev server (http://localhost:3000)
mise run start

# Install pre-commit hooks
prek install

# Run tests
cargo test

# Lint
cargo clippy

# Format
cargo fmt
```
