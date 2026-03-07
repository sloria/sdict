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

## CSS

CSS lives inline in `templates/base.html`, organized using [CUBE CSS](https://cube.fyi) with sections separated by `/* === Layer === */` comments:

- **Global** — tokens (custom properties), reset, base element styles (`body`, `a`, `footer`)
- **Compositions** — reusable layout primitives (`.wrapper`, `.flow`, `.cluster`)
- **Utilities** — single-purpose classes (`.color-secondary`, `.text-s`, `.font-semibold`, `.indent`, etc.)
- **Blocks** — component-specific styles (`.search-form`, `.hero`, `.sense`, `.filter-tag`, etc.). Keep blocks small; most styling should come from compositions and utilities.
- **Exceptions** — state variations via `data-` attributes (e.g. `data-state="active"`, `data-size="small"`)

When adding new styles, place them in the appropriate CUBE layer. Prefer utilities and compositions over adding properties to blocks.

## Testing

Tests use saved HTML fixtures in `tests/fixtures/` and `wiremock` for HTTP mocking. The `translate()` function accepts a configurable `base_url` so tests can point at mock servers instead of hitting SpanishDict.
