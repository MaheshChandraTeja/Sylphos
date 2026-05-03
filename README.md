# Sylphos
### Native Rust Browser Engine Lab

<p align="center">
  <img alt="Language" src="https://img.shields.io/badge/language-Rust-000000?logo=rust">
  <img alt="Edition" src="https://img.shields.io/badge/edition-2021-blue">
  <img alt="Workspace" src="https://img.shields.io/badge/workspace-6%20crates-4C8BF5">
  <img alt="Graphics" src="https://img.shields.io/badge/graphics-wgpu%200.20-6A5ACD">
  <img alt="Windowing" src="https://img.shields.io/badge/windowing-winit%200.30-2E8B57">
  <img alt="Network" src="https://img.shields.io/badge/network-hyper%20%2B%20rustls-111111">
  <img alt="Privacy" src="https://img.shields.io/badge/privacy-local--first-brightgreen">
</p>

Sylphos is a native Rust browser-engine prototype built around a direct pipeline:
fetch the document, parse the HTML, extract a presentation model, apply the
currently supported style and script effects, and render the result in a native
GPU-backed shell.

It is not a WebView wrapper.
It is not Chromium in a lab coat.
It is not pretending that a research browser and the full web platform are the
same thing.

It is a local-first systems project for building the browser pieces directly:
networking, tolerant parsing, presentation extraction, CSS-lite, forms, DOM
mutation, script-effect handling, media/canvas/worker signals, resource caching,
tabs, history, reflow, paint planning, and native rendering.

No fake browser magic.
No mystery cloud service.
No "it works because a giant engine is hiding under the table."

Just the browser pipeline, built where it can be inspected.

---

## Overview

Sylphos is organized around one idea:

> Build a browser by making each layer explicit enough to test, inspect, and
> replace.

The current workspace contains:

  * a native desktop app shell
  * an async HTTP/TLS fetch crate
  * a small HTML tokenizer/parser/serializer
  * a presentation crate with CSS-lite, layout, paint, forms, mutation, and reflow
  * a research JavaScript subset VM called `syljs`
  * app-side script discovery and Web Platform effect handling
  * GPU text/image rendering helpers
  * local cache and history storage
  * tests covering the active crates

Sylphos is useful for:

  * browser-engine research
  * native rendering experiments
  * HTML/CSS extraction work
  * deterministic layout and paint tests
  * script/runtime boundary experiments
  * local-first web tooling
  * understanding what a browser does before the enormous parts arrive

That last point matters. Browser engines are not large by accident. Sylphos is
where the smaller pieces are made visible before they become a cathedral of
edge cases.

---

## Table of Contents

  * What Is Sylphos?
  * Current Development State
  * Design Philosophy
  * Feature Matrix
  * Architecture
  * Runtime Flow
  * Workspace Map
  * Browser Shell
  * Presentation Layer
  * JavaScript and Web Platform Work
  * SylJS
  * Installation
  * Running
  * CLI
  * Development Workflow
  * Quality Gates
  * Cache and Local State
  * Security and Privacy
  * Current Boundaries
  * Repository Layout
  * Authors
  * License
  * Closing Note

---

## What Is Sylphos?

Sylphos is a native browser shell and browser-engine research workspace.

It starts with the classical browser pipeline:

    URL
      -> fetch
      -> HTML parse
      -> document extraction
      -> style/script/resource processing
      -> layout
      -> paint plan
      -> native GPU frame

The project is intentionally split into crates so each part can be tested
without needing the whole app to be open in a window.

The app is native-first. The renderer is `wgpu`. The window and input layer are
`winit`. The network path is built on `hyper`, `hyper-util`, and `hyper-rustls`.
The presentation model is owned by the `present` crate instead of being smeared
through the UI shell like wet cement.

That separation is not decorative. It is the difference between a browser
project and a pile of callbacks wearing a toolbar.

---

## Current Development State

This README describes the workspace as it exists now.

Future development is intentionally not documented here yet. When the next
round of work lands, this file can be revised again. A stale roadmap is worse
than no roadmap; it becomes a motivational poster for code that does not exist.

Current state:

  * the Rust workspace builds as six crates
  * the native app crate is `sylphos`
  * the presentation crate includes forms, mutation, CSSOM-lite, selector work,
    invalidation, incremental reflow, and paint planning
  * the browser shell has tabs, history, cache, resources, stylesheets, forms,
    images, and DOM interaction controllers
  * the app-side JavaScript layer captures bounded script effects for DOM,
    CSSOM, storage, cookies, history, fetch/XHR, media, canvas, and workers
  * the separate `syljs` crate implements a research JavaScript subset VM with
    tests for DOM, CSSOM, Web APIs, media, Canvas 2D, workers, transferables,
    and benchmarks
  * the full workspace currently passes check, Clippy, and tests

The important word is "current." This is not a promise that the browser is
complete. It is a record of the present engineering surface.

---

## Design Philosophy

Principle | Meaning
--- | ---
Local-first | Sylphos runs locally. Fetching happens because navigation asks for it, not because telemetry got bored.
Layered | Fetching, parsing, presentation, scripting, layout, and rendering have separate boundaries.
Inspectable | Intermediate data structures should be plain enough to test and reason about.
Honest | Unsupported platform behavior should be visible, not faked into a success story.
Deterministic where possible | Parser output, paint plans, layout behavior, and runtime summaries should be testable.
Native | The app is a desktop browser shell, not a web app pretending to inspect the web.
Research-friendly | Experimental modules are welcome, but they should say what they are.

Sylphos is serious about browser systems, but not solemn about them. The web is
already complicated enough without adding ceremonial abstraction.

---

## Feature Matrix

Area | Current capability | Status
--- | --- | ---
Workspace | `util`, `fetch`, `html_mvp`, `present`, `syljs`, `app` | Active
Native app | `winit` desktop window, input handling, toolbar, tabs | Active
Rendering | `wgpu` surface, font atlas, image atlas, paint command rendering | Active
Fetch | HTTP/HTTPS, redirects, compression, limits, browser-like headers | Active
Cache | HTML, image, stylesheet, font, script buckets with TTL policy | Active
History | Successful visit recording | Active
Tabs | Open, switch, close, back, forward, reload | Active
HTML MVP | Tokenizer, parser, DOM, serializer, entity decoding | Active
Presentation | DOM extraction, text, headings, links, images, forms | Active
CSS-lite | Rule parsing, selector matching, computed paint styles | Active
Stylesheets | External stylesheet loading and `@import` discovery | Active
Forms | Focus, edit, GET submission URL construction | Active
Hit testing | Links and form controls | Active
Mutation | Mutable DOM model and dirty flags | Active
Reflow | Incremental reflow engine and dirty regions | Active
App script layer | Bounded script discovery and effect capture | Active
Web Platform effects | Storage, cookies, history, fetch/XHR, media/canvas/worker signals | Active
SylJS | JavaScript subset frontend, bytecode VM, event loop, DOM/CSSOM/Web API hosts | Active research crate
Benchmarks | Synthetic YouTube-like SylJS benchmark harness | Active research crate
Full browser compatibility | Complete HTML/CSS/JS platform parity | Not claimed

Legend:

    Active = implemented in the current workspace surface
    Active research crate = implemented and tested, but still a research VM surface
    Not claimed = deliberately not represented as complete browser support

---

## Architecture

Sylphos is layered from network and parsing up to app rendering.

```text
┌───────────────────────────────────────────────────────────────┐
│ Native App Shell: window, toolbar, tabs, input, navigation     │
├───────────────────────────────────────────────────────────────┤
│ App Browser Services: cache, history, resources, images        │
├───────────────────────────────────────────────────────────────┤
│ App Script Effects: DOM, CSSOM, storage, cookies, media        │
├───────────────────────────────────────────────────────────────┤
│ Presentation: extraction, selectors, forms, mutation, reflow   │
├───────────────────────────────────────────────────────────────┤
│ HTML MVP: tokenizer, parser, DOM, serializer                   │
├───────────────────────────────────────────────────────────────┤
│ Fetch: HTTP/TLS, redirects, decompression, limits              │
├───────────────────────────────────────────────────────────────┤
│ Utilities: tracing, shared runtime setup                       │
└───────────────────────────────────────────────────────────────┘
```

`syljs` sits beside that app path as a research runtime crate:

```text
┌───────────────────────────────────────────────────────────────┐
│ SylJS Research Runtime                                        │
├───────────────────────────────────────────────────────────────┤
│ Lexer / Parser / AST / Bytecode Compiler                      │
├───────────────────────────────────────────────────────────────┤
│ VM / Values / Promise-lite / Event Loop                       │
├───────────────────────────────────────────────────────────────┤
│ Host APIs: DOM, CSSOM, Web API, Media, Canvas, Worker          │
├───────────────────────────────────────────────────────────────┤
│ Benchmarks and isolated-worker experiments                     │
└───────────────────────────────────────────────────────────────┘
```

The separation is useful because the app can keep its bounded browser-effect
pipeline while the research VM grows under test pressure.

---

## Runtime Flow

The native app navigation path is:

```text
User URL
  |
  v
browser::normalize_user_url()
  |
  v
ResourceScheduler::fetch_text(document)
  |
  v
CacheStore / fetch::get()
  |
  v
html_mvp::parse()
  |
  v
present::extract_render_document()
  |
  v
stylesheet loading + CSSOM-lite application
  |
  v
app-side script discovery and bounded effect capture
  |
  v
image discovery, fetch, decode
  |
  v
present::layout_document()
  |
  v
present::build_paint_plan_from_layout()
  |
  v
render mesh/font/image atlas update
  |
  v
wgpu frame
```

That flow is intentionally boring. Boring pipelines are easier to debug.
Exciting pipelines are usually just missing error boundaries.

---

## Workspace Map

Crate | Role
--- | ---
`crates/app` | Native browser shell and orchestration
`crates/fetch` | Async HTTP/TLS client with redirects, decoding, and limits
`crates/html_mvp` | Minimal HTML tokenizer, parser, DOM, serializer, entity decoding
`crates/present` | Presentation model, CSS-lite, layout, forms, mutation, reflow, paint
`crates/syljs` | JavaScript subset VM and Web Platform research runtime
`crates/util` | Tracing setup and shared utility exports

The root `Cargo.toml` owns the shared dependency versions and lint policy. The
workspace denies unsafe code and treats unused code paths seriously, except
where an explicit research-module allowance is scoped locally.

---

## Browser Shell

The app shell currently provides:

  * native window creation through `winit`
  * GPU rendering through `wgpu`
  * URL bar input
  * back, forward, reload, new-tab, close-tab, and switch-tab behavior
  * tab-local navigation state
  * persistent history recording
  * cache policy setup
  * page loading on a Tokio runtime
  * async pipeline messages back to the UI thread
  * cursor changes for hit-tested links and controls
  * toolbar and page-surface paint composition

The shell is not just a demo window. It owns the browser workflow boundary:
input enters here, navigation starts here, and rendering lands here.

---

## Presentation Layer

The `present` crate currently covers:

  * render-document types
  * text, headings, links, images, and fallback blocks
  * form extraction and form submission data
  * CSS-lite parsing
  * selector matching and specificity
  * box model primitives
  * inline flow and wrapping
  * viewport-aware layout
  * hit regions for links and form controls
  * mutable DOM runtime structures
  * dirty flags and invalidation sets
  * CSSOM-lite mutations and style updates
  * incremental reflow and dirty-region calculation
  * deterministic paint command generation

The presentation crate is where Sylphos tries to avoid the most common browser
prototype mistake: letting rendering logic become the document model because it
was nearby and had a mutable reference.

---

## JavaScript and Web Platform Work

The app-side JavaScript layer is deliberately bounded.

It currently includes modules for:

  * script discovery and loading
  * console capture
  * DOM binding effect capture
  * CSSOM effect capture
  * storage state
  * cookie handling
  * History API state
  * fetch and XHR effect detection
  * timer/event-loop summaries
  * media, canvas, worker, and MediaSource signal detection

This is not a full JavaScript engine in the app shell.

It is a browser-service boundary for recognizing and applying selected effects
that matter to the current presentation pipeline. That is much less glamorous
than saying "full JS support," but it has the advantage of being true.

---

## SylJS

`crates/syljs` is the research JavaScript subset runtime.

It currently includes:

  * lexer, parser, AST, and diagnostics
  * bytecode compiler
  * stack VM
  * runtime values and heap objects
  * Promise-lite support
  * event loop with timers, microtasks, and RAF
  * DOM host bindings
  * CSSOM host bindings
  * Web API host bindings
  * media and MediaSource simulation
  * Canvas 2D command recording
  * Worker-lite and isolated-worker paths
  * transferables
  * synthetic YouTube-like benchmark generation and metrics

SylJS is not V8. It is not trying to be V8 with a smaller logo.

It exists so Sylphos can study browser-script interactions in a runtime that is
small enough to understand and strict enough to test.

---

## Installation

### Prerequisites

Install:

  * Rust 1.74 or newer
  * a desktop environment supported by `winit`
  * a GPU/driver stack supported by `wgpu`
  * network access for live URL fetching

The workspace uses the Rust 2021 edition.

### Build

```bash
cargo build --workspace
```

### Check

```bash
cargo check --workspace --all-targets
```

---

## Running

Run with the default URL:

```bash
cargo run -p sylphos
```

Run with an explicit URL:

```bash
cargo run -p sylphos -- --url https://example.com
```

Run with cache disabled:

```bash
cargo run -p sylphos -- --url https://example.com --disable-cache
```

Clear cache before startup:

```bash
cargo run -p sylphos -- --url https://example.com --clear-cache
```

Use a custom cache directory:

```bash
cargo run -p sylphos -- --cache-dir ./tmp/sylphos-cache
```

---

## CLI

```text
sylphos [OPTIONS]

Options:
  --url <URL>           Initial page URL. Defaults to https://example.com
  --disable-cache       Skip cache reads and writes
  --clear-cache         Clear the configured cache before startup
  --cache-dir <PATH>    Use a custom cache directory
```

Plain domains are normalized by the browser navigation layer. Unsupported link
schemes such as `javascript:` are rejected instead of being treated as cute
little navigation adventures.

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

```text
cargo fmt --all
cargo clippy --workspace --all-features -- -D warnings
cargo test --workspace --all-features -- --nocapture
```

For format-check mode:

```bash
CHECK_MODE=true bash scripts/dev.sh
```

In CI, the scripts automatically use format check mode.

---

## Quality Gates

The current workspace is expected to pass:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets
cargo test --workspace
```

The `fetch` crate has a live network test marked `#[ignore]`. Normal test runs
skip it on purpose. Unit tests should not depend on the public internet having
a good day.

---

## Cache and Local State

Sylphos stores browser data locally.

Current cache buckets include:

  * HTML/text documents
  * external stylesheets
  * images
  * fonts
  * scripts

The cache has:

  * TTL policy per resource kind
  * in-memory byte limits
  * disk entry byte limits
  * metadata validation
  * custom root support through `--cache-dir`
  * explicit clearing through `--clear-cache`
  * full bypass through `--disable-cache`

Current default cache root behavior is platform-aware:

Platform | Default
--- | ---
Windows | `%LOCALAPPDATA%\Kairais\Syphos\cache`
macOS | `~/Library/Caches/Kairais/Syphos`
Linux | `$XDG_CACHE_HOME/syphos` or `~/.cache/syphos`
Fallback | `.syphos-cache`

The directory spelling above matches the current code. If that naming is
normalized later, this README should move with it.

---

## Security and Privacy

Current privacy posture:

  * no telemetry
  * no analytics pipeline
  * no cloud dependency
  * local cache and history only
  * user navigation triggers document fetches
  * app-side script effects are bounded
  * cache can be disabled or cleared from the CLI

This is still a browser-like program, so it makes network requests when asked
to navigate. It is local-first, not offline-only.

---

## Current Boundaries

Sylphos currently does not claim:

  * full HTML5 tree-construction parity
  * full CSS cascade/layout compatibility
  * full JavaScript engine compatibility in the app shell
  * full DOM API coverage
  * full accessibility tree support
  * full browser security sandboxing
  * production-grade site compatibility

The point is not to pretend these are small missing checkboxes.

The point is to build enough browser machinery that the missing pieces have a
clear place to land.

---

## Repository Layout

```text
Sylphos
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── scripts
│   ├── dev.ps1
│   └── dev.sh
└── crates
    ├── app
    │   ├── src
    │   │   ├── browser
    │   │   ├── js
    │   │   ├── render
    │   │   ├── main.rs
    │   │   └── state.rs
    │   └── Cargo.toml
    ├── fetch
    ├── html_mvp
    ├── present
    ├── syljs
    └── util
```

The `target` directory is build output. It is not architecture. It is where the
compiler stores its receipts.

---

## Authors

Built by:

  * Mahesh Chandra Teja Garnepudi
  * Sagarika Srivastava

Organization:

  * Kairais Tech

---

## Closing Note

Sylphos is a browser-engine lab for people who want to see the machinery.

Fetch the page.
Parse the document.
Extract the presentation.
Apply the bounded platform effects.
Lay it out.
Paint it.
Render it natively.

That is the current shape of the project.

Everything else can wait until it exists.
