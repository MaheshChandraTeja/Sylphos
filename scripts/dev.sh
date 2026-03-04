set -euo pipefail

IS_CI="${CI:-}"
CHECK_MODE="${CHECK_MODE:-}"

fmt() {
  if [[ -n "$IS_CI" || -n "$CHECK_MODE" ]]; then
    echo "==> cargo fmt --all -- --check"
    cargo fmt --all -- --check
  else
    echo "==> cargo fmt --all"
    cargo fmt --all
  fi
}

clippy() {
  echo "==> cargo clippy --workspace --all-features -- -D warnings"
  cargo clippy --workspace --all-features -- -D warnings
}

test_all() {
  echo "==> cargo test --workspace --all-features -- --nocapture"
  RUST_BACKTRACE=1 cargo test --workspace --all-features -- --nocapture
}

main() {
  fmt
  clippy
  test_all
  echo "✓ All good."
}

main "$@"
