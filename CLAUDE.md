# CLAUDE.md

## Build & test

- `cargo test` — run all tests (unit + integration)
- `cargo clippy` — lint
- `cargo fmt` — format
- `cargo run` — start dev server on port 3000

## Architecture

Axum web server with Askama server-side templates. No JavaScript.

- `src/lib.rs` — routes (`GET /`, `POST /search`, `GET /translate/{term}`), Askama template structs
- `src/spanishdict.rs` — scrapes SpanishDict HTML, extracts `SD_COMPONENT_DATA` JSON from `<script>` tags, parses definitions and corpus examples
- `templates/` — Askama HTML templates extending `base.html`

## How scraping works

SpanishDict has no API. The app fetches two pages per lookup:
1. `/translate/{term}` — definitions and per-definition examples (from `sdDictionaryResultsProps.entry.neodict`)
2. `/examples/{term}?lang=es` — corpus example sentences (from `explorationResponseFromServerEs.data.data.sentences`)

Both pages embed a `window.SD_COMPONENT_DATA = {...};` JSON blob in a `<script>` tag, which is extracted via regex.

## Testing

Tests use saved HTML fixtures in `tests/fixtures/` and `wiremock` for HTTP mocking. The `translate()` function accepts a configurable `base_url` so tests can point at mock servers instead of hitting SpanishDict.
