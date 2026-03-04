<h1 align="center">Sylphos</h1>

<p align="center">
  <b>Fetch → Parse → Present</b><br/>
  A Rust workspace that fetches live HTML, normalizes it with a minimal DOM stack, and renders a GPU-backed app that reacts to page theme color.
</p>

<p align="center">
  <img alt="Rust" src="https://img.shields.io/badge/language-Rust-000000?logo=rust">
  <img alt="Edition" src="https://img.shields.io/badge/edition-2021-blue">
  <img alt="wgpu" src="https://img.shields.io/badge/graphics-wgpu%200.20-4C8BF5">
  <img alt="winit" src="https://img.shields.io/badge/windowing-winit%200.30-6A5ACD">
  <img alt="network" src="https://img.shields.io/badge/network-hyper%20%2B%20rustls-2E8B57">
  <img alt="status" src="https://img.shields.io/badge/status-active%20prototype-orange">
</p>

<p align="center">
  <a href="#-quick-start">Quick Start</a> •
  <a href="#-workspace-architecture">Architecture</a> •
  <a href="#-crate-breakdown">Crates</a> •
  <a href="#-development-workflow">Dev Workflow</a> •
  <a href="#-roadmap">Roadmap</a>
</p>

---

## 🚀 What Sylphos Is

Sylphos is a multi-crate Rust workspace focused on a clean end-to-end pipeline:

1. **Fetch** a URL over hardened HTTP/TLS.
2. **Parse** the response into a minimal HTML-ish DOM.
3. **Present** the result in a native window with GPU rendering.

The current app updates the render clear color using `<meta name="theme-color" ...>` when available.

---

## 🧱 Workspace Architecture

```text
Sylphos
├── crates
│   ├── app       # Winit + wgpu app, orchestration layer
│   ├── fetch     # Hyper + rustls HTTP client wrapper
│   ├── html_mvp  # Tokenizer, parser, serializer, DOM equality
│   └── util      # Tracing bootstrap + shared prelude
├── scripts
│   ├── dev.sh    # fmt + clippy + test (Unix)
│   └── dev.ps1   # fmt + clippy + test (PowerShell)
└── Cargo.toml    # Workspace + shared deps/lints/profiles
```

### 🔄 Runtime Flow

```text
URL
 ↓
fetch::get()
 ↓
Response text (size-limited, decoded)
 ↓
html_mvp::parse()
 ↓
DOM inspection (theme-color)
 ↓
wgpu render loop (clear color update)
```

---

## 📦 Crate Breakdown

### `crates/fetch`
- Async HTTP client built on **hyper 1.x + hyper-rustls**
- Redirect handling with a max-hop cap
- `gzip` / `deflate` response decoding
- Header-size and body-size safeguards
- Request timeout and lazy global client initialization

### `crates/html_mvp`
- Lightweight tokenizer for a practical HTML-ish subset
- Stack-based DOM builder
- Deterministic serializer
- Structural DOM equality helpers (`dom_eq`)
- Property tests for parse/serialize round-trips

### `crates/app`
- Native app shell using **winit 0.30** event model
- GPU rendering with **wgpu 0.20**
- Async orchestration using a Tokio runtime
- Integrates `fetch` + `html_mvp` pipeline into a render loop

### `crates/util`
- One-time tracing initialization
- Env-driven log formatting (`RUST_LOG`, `LOG_FORMAT`, `RUST_LOG_FORMAT`, `CI`)
- Shared error/logging prelude for workspace crates

---

## ⚡ Quick Start

### 1) Prerequisites

- Rust toolchain (workspace targets Rust 1.74+)
- A GPU/driver setup compatible with `wgpu`
- Network access for fetching remote URLs

### 2) Build

```bash
cargo build --workspace
```

### 3) Run the app

```bash
cargo run -p myapp -- --url https://example.com
```

If no URL is passed, the app defaults to `https://example.com`.

---

## 🛠️ Development Workflow

Run the full local quality pass:

```bash
# Unix/macOS
bash scripts/dev.sh
```

```powershell
# Windows PowerShell
./scripts/dev.ps1
```

These run:

- `cargo fmt --all` (or `--check` in CI/check mode)
- `cargo clippy --workspace --all-features -- -D warnings`
- `cargo test --workspace --all-features -- --nocapture`

---

## ⚙️ Configuration Notes

### CLI
- `--url <VALUE>`: target URL to fetch and parse

### Environment
- `RUST_LOG`: trace filter (for example `info,hyper=warn`)
- `LOG_FORMAT=json` or `RUST_LOG_FORMAT=json`: force JSON logs
- `CI=true`: disables ANSI and uses CI-friendly logging behavior
- `CHECK_MODE=true`: scripts run fmt in check mode

---

## 🧪 Testing and Validation

```bash
cargo test --workspace --all-features -- --nocapture
```

`fetch` includes a live network test marked `#[ignore]`, so it is skipped by default unless explicitly enabled.

---

## 🗺️ Roadmap

- [ ] Improve `app` rendering path and API-version stabilization
- [ ] Expand HTML parser correctness and compatibility surface
- [ ] Add richer present-layer visuals driven by parsed DOM
- [ ] Add benchmark and profiling harnesses for fetch/parse pipeline
- [ ] Harden integration tests around full fetch → parse → present flow

---

## 🤝 Contributing

Contributions are welcome. Prefer small, focused PRs with:

- clear intent
- tests for behavior changes
- formatting/linting pass before submission

---

## Authors

**Mahesh Chandra Teja Garnepudi**  
**Sagarika Srivastava**  

Built at **Kairais Tech**

---

## License

Private project for now. Licensing details will be published later.
