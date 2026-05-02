<h1 align="center">Sylphos</h1>

### Native Rust Browser Shell for Fetching, Parsing, Presenting, and Inspecting the Web

<p align="center">
  <img alt="Build" src="https://img.shields.io/badge/build-clean-brightgreen">
  <img alt="Language" src="https://img.shields.io/badge/language-Rust-000000?logo=rust">
  <img alt="Edition" src="https://img.shields.io/badge/edition-2021-blue">
  <img alt="Graphics" src="https://img.shields.io/badge/graphics-wgpu%200.20-4C8BF5">
  <img alt="Windowing" src="https://img.shields.io/badge/windowing-winit%200.30-6A5ACD">
  <img alt="Network" src="https://img.shields.io/badge/network-hyper%20%2B%20rustls-2E8B57">
  <img alt="Privacy" src="https://img.shields.io/badge/privacy-local--first-111111">
</p>

Sylphos is a native Rust browser prototype built around a clear systems pipeline: fetch a live URL, parse the response into a small DOM, extract a presentation model, and render it with a GPU-backed desktop shell.

It is not trying to be Chromium in a trench coat.
It does not execute arbitrary page JavaScript.
It does not pretend a simplified renderer is a complete web platform.

It is a focused, local-first browser lab for building the primitives: networking, HTML recovery, deterministic presentation, cache behavior, tabs, history, image decoding, and native rendering.

---

## Features

### Browser Shell

- Native desktop window built with `winit`
- GPU-backed rendering through `wgpu`
- URL bar with keyboard navigation
- Back, forward, reload, and new-tab controls
- Multi-tab state with active-tab switching and tab close behavior
- Persistent visit history
- Runtime window icon and Windows executable resource icon
- App naming normalized to `Sylphos` / `sylphos.exe`

### Fetch Pipeline

- Async HTTP client built on `hyper` and `hyper-rustls`
- Browser-like request headers for better compatibility with modern sites
- HTTPS and HTTP support
- Redirect handling with a maximum redirect cap
- `gzip` and `deflate` response decoding
- Request timeout, body-size, and header-size guards
- Browser HTML cache with TTL and versioned metadata
- Image byte cache with separate TTL and byte limits

### HTML MVP

- Minimal tokenizer, parser, DOM, and serializer
- Forgiving parse behavior for malformed or partial HTML
- Named entity decoding
- Numeric entity decoding, including decimal and hexadecimal forms
- Common entity support such as `&nbsp;`, `&copy;`, and `&reg;`
- Deterministic serialization for tests and diagnostics
- Property tests for parse/serialize round trips

### Presentation Layer

- DOM-to-render-document extraction
- Title and theme-color extraction
- Lightweight CSS parsing for selected page-level styles
- Text blocks, headings, links, image placeholders, and generic fallback blocks
- Legacy HTML container handling for pages that still emit older markup
- Link hit-testing for page-local interactions
- Deterministic layout and paint-plan generation

### Rendering

- Font atlas based text rendering
- Image decode and atlas upload path
- Viewport-aware layout
- Page and browser-chrome composition
- Recoverable `wgpu::SurfaceError::Lost` and `Outdated` handling
- Quiet default logging for noisy GPU backend diagnostics

---

## Design Principles

- Keep networking, parsing, presentation, and rendering separated.
- Treat unsupported browser behavior honestly instead of faking it.
- Prefer deterministic intermediate models that can be tested.
- Keep platform-specific behavior isolated in the app shell.
- Make failure states visible in logs without flooding normal runs.
- Build small, inspectable systems before attempting full browser complexity.

---

## Workspace Architecture

```text
Sylphos
├── crates
│   ├── app
│   │   ├── assets        # Application icon assets
│   │   ├── browser       # Tabs, history, navigation, cache, image loading, chrome state
│   │   ├── render        # Meshes, font atlas, image atlas, GPU draw helpers
│   │   ├── build.rs      # Windows executable icon/resource metadata
│   │   ├── main.rs       # App lifecycle and browser pipeline orchestration
│   │   └── state.rs      # wgpu surface, pipeline, buffers, render loop
│   ├── fetch             # HTTP/TLS fetch client, redirects, decoding, limits
│   ├── html_mvp          # Tokenizer, parser, DOM, serializer, property tests
│   ├── present           # DOM extraction, CSS-lite, layout, paint planning, hit testing
│   └── util              # Tracing initialization and shared prelude
├── scripts
│   ├── dev.ps1           # Windows quality pass
│   └── dev.sh            # Unix quality pass
├── Cargo.toml            # Workspace members, shared deps, lint policy
└── rust-toolchain.toml   # Toolchain pin
```

---

## Runtime Flow

```text
User URL
  |
  v
browser::normalize_user_url()
  |
  v
CacheStore::get_or_fetch_text()
  |
  v
fetch::get()
  |
  v
html_mvp::parse()
  |
  v
present::extract_render_document()
  |
  v
present::build_paint_plan()
  |
  v
app::render mesh/font/image atlas build
  |
  v
wgpu frame
```

---

## Core Modules

| Module | Role | Notes |
| --- | --- | --- |
| `crates/app` | Native browser shell | Window lifecycle, tabs, history, cache integration, renderer orchestration |
| `crates/app/src/browser` | Browser state | URL normalization, navigation history, tab manager, persistent cache, chrome actions, image loading |
| `crates/app/src/render` | Draw backend helpers | Mesh construction, font atlas, image atlas, texture-backed drawing |
| `crates/fetch` | Network layer | Hyper client, rustls TLS, redirects, compression decoding, request/body/header limits |
| `crates/html_mvp` | HTML subset | DOM, tokenizer, parser, serializer, entity decoding, round-trip tests |
| `crates/present` | Presentation model | DOM extraction, CSS-lite, layout, paint plans, link hit-testing |
| `crates/util` | Shared runtime utilities | Tracing setup and common error/logging exports |

---

## Platform Support

| Platform | Status | Notes |
| --- | --- | --- |
| Windows | Active target | Windows executable resources and runtime window icon are configured |
| Linux | Expected with compatible GPU stack | Uses cross-platform `winit` and `wgpu` paths |
| macOS | Expected with compatible GPU stack | Not the current primary validation environment |
| Web/WASM | Not supported yet | Some limits are scaffolded, but the app is native-first |

---

## App Screens

| Screen / Surface | Purpose |
| --- | --- |
| Tab strip | Shows open tabs, active tab, loading state, and close controls |
| Navigation bar | URL input, back, forward, reload, and new-tab actions |
| Page surface | Displays extracted document blocks from fetched HTML |
| Link hover state | Updates cursor and toolbar status when hovering extracted links |
| History store | Persists successful visits for later browser workflow expansion |
| Cache store | Reuses HTML and image responses within configured TTLs |

---

## Getting Started

### Prerequisites

- Rust 1.74 or newer
- Windows, Linux, or macOS desktop environment
- GPU/driver support compatible with `wgpu`
- Network access for live URL fetches

### Build

```bash
cargo build --workspace
```

### Run

```bash
cargo run -p sylphos -- --url https://example.com
```

### Run Against Google With a Fresh Page Cache

```bash
cargo run -p sylphos -- --url https://google.com --clear-cache
```

The app defaults to `https://example.com` when no URL is provided.

---

## CLI

```text
sylphos [OPTIONS]

Options:
  --url <URL>           Initial page URL. Defaults to https://example.com
  --disable-cache       Fetch through the network and skip cache reads/writes
  --clear-cache         Clear the Sylphos cache before startup
  --cache-dir <PATH>    Use a custom cache directory
```

---

## Development Workflow

### Windows

```powershell
./scripts/dev.ps1
```

### Unix / macOS

```bash
bash scripts/dev.sh
```

The scripts run:

- `cargo fmt --all`
- `cargo clippy --workspace --all-features -- -D warnings`
- `cargo test --workspace --all-features -- --nocapture`

For CI-style format checking:

```bash
CHECK_MODE=true bash scripts/dev.sh
```

---

## Quality Gates

The current workspace is expected to pass:

```bash
cargo fmt --check
cargo build
cargo test
cargo clippy --workspace --all-targets
```

The `fetch` crate includes a live network test marked `#[ignore]`; it is intentionally skipped during normal test runs.

---

## Configuration

### Logging

- `RUST_LOG`: tracing filter, for example `sylphos=info`
- `LOG_FORMAT=json`: force JSON logs
- `RUST_LOG_FORMAT=json`: force JSON logs
- `CI=true`: disable ANSI output and use CI-friendly logging

By default, Sylphos keeps common noisy GPU backend warnings quiet while still allowing explicit `RUST_LOG` overrides for targeted debugging.

### Cache Locations

Default cache locations are platform-aware:

- Windows: `%LOCALAPPDATA%\Kairais\Sylphos\cache`
- macOS: `~/Library/Caches/Kairais/Sylphos`
- Linux: `$XDG_CACHE_HOME/sylphos` or `~/.cache/sylphos`
- Fallback: `.sylphos-cache`

---

## Security and Privacy

- No telemetry
- No cloud dependency
- No background upload pipeline
- Fetching is initiated by user navigation
- HTML and images are cached locally
- Cache can be disabled with `--disable-cache`
- Cache can be cleared with `--clear-cache`

The project is local-first by design. Network requests happen because the browser is asked to navigate, not because an analytics subsystem woke up.

---

## Tech Stack

- Rust 2021
- `winit` for native windowing and input
- `wgpu` for GPU rendering
- `hyper`, `hyper-util`, and `hyper-rustls` for HTTP/TLS
- `tokio` for async runtime work
- `image` for image decoding
- `fontdue` for font rasterization
- `serde` and `serde_json` for cache metadata/history storage
- `proptest` and `insta` for parser/serializer validation

---

## Current Browser Behavior

Sylphos renders a simplified static presentation of fetched HTML. That is intentional.

Supported today:

- Fetching HTML over HTTP/HTTPS
- Following redirects
- Decoding compressed responses
- Parsing a practical HTML subset
- Extracting visible text, headings, links, images, titles, theme color, and some CSS
- Rendering extracted content in a native GPU surface
- Navigating links discovered in the extracted document

Not supported yet:

- JavaScript execution
- Full CSS cascade/layout
- Forms and search submission
- Web fonts as page resources
- DOM mutation
- Cookies/session storage
- Full accessibility tree
- Full HTML5 parsing algorithm

For pages like Google, Sylphos can fetch and present the static HTML response, but it does not reproduce the full interactive browser experience that depends on JavaScript, cookies, and a complete layout engine.

---

## Research Motivation

Sylphos is motivated by a practical systems question:

> How much of a useful browser-like experience can be built from small, testable, local-first components before adopting the full complexity of a modern browser engine?

Modern browser engines are enormous because the web platform is enormous. Sylphos takes the opposite route: isolate the pieces, make each piece deterministic where possible, and use the project as a research and engineering surface for understanding the real boundaries between fetching, parsing, presentation, and rendering.

The project is useful for investigating:

- Tolerant parsing of imperfect live HTML
- Deterministic serialization and round-trip behavior
- Minimal render-document extraction from arbitrary pages
- Cache behavior and offline-friendly document/image reuse
- GPU text and image rendering without a heavyweight UI framework
- Browser-shell state management with tabs, history, and navigation
- Honest degradation when a page requires unsupported platform features

---

## Recent Changes

- Renamed the app path and user-facing name to `Sylphos`
- Added Windows executable resource metadata and icon embedding
- Added runtime window-frame icon support
- Added browser-like request headers for better modern-site compatibility
- Added cache metadata versioning to invalidate stale cached document bodies
- Added numeric HTML entity decoding
- Improved legacy HTML container extraction
- Added persistent history, tabs, cache, image discovery, and image decode paths
- Added recoverable handling for `wgpu::SurfaceError::Outdated`
- Cleaned current build, test, and Clippy warnings

---

## Roadmap

- Search/form submission from extracted form controls
- Cookie and session handling
- Scrollable page surface
- Better Google/basic-search page presentation
- More complete HTML tree construction behavior
- More CSS-lite selectors and inherited style handling
- Better text shaping and font fallback
- Persistent tab/session restore
- Download and resource inspection panel
- Screenshot-based visual regression tests
- Cross-platform packaging

---

## About

Built by:

- Mahesh Chandra Teja Garnepudi
- Sagarika Srivastava

Organization:

- Kairais Tech

---

## License

Private project for now. Licensing details will be published later.

---

## Philosophy

Browser work should be observable, testable, and honest about what is implemented.

Sylphos exists to build those pieces directly: not a webview wrapper, not a fake full browser, and not a black box. Just the pipeline, the renderer, and the parts that make the web understandable one layer at a time.
