#![deny(unsafe_code)]

use once_cell::sync::OnceCell;
use std::env;
use std::io;
use tracing_subscriber::{util::SubscriberInitExt, EnvFilter};

static INSTALLED: OnceCell<()> = OnceCell::new();

pub fn init_tracing() {
    INSTALLED.get_or_init(|| {
        let is_ci = env_flag("CI");
        let force_json = is_ci || env_eq("LOG_FORMAT", "json") || env_eq("RUST_LOG_FORMAT", "json");

        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,hyper=warn,reqwest=warn,tokio_util=warn"));

        let time = tracing_subscriber::fmt::time::UtcTime::rfc_3339();

        if force_json {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .event_format(tracing_subscriber::fmt::format().json().flatten_event(true))
                .with_timer(time)
                .with_target(true)
                .with_level(true)
                .with_ansi(false)
                .with_writer(io::stderr)
                .finish()
                .init();
        } else {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_timer(time)
                .with_target(true)
                .with_level(true)
                .with_ansi(!is_ci)
                .with_writer(io::stderr)
                .finish()
                .init();
        }
    });
}

pub mod prelude {
    pub use anyhow::{anyhow, bail, ensure, Context, Result};
    pub use thiserror::Error;

    pub use tracing::instrument;
    pub use tracing::{debug, error, info, trace, warn, Level};
}

fn env_flag(name: &str) -> bool {
    matches!(
        env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("True")
    )
}

fn env_eq(name: &str, val: &str) -> bool {
    env::var(name)
        .map(|v| v.eq_ignore_ascii_case(val))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        init_tracing();
        init_tracing();
        tracing::info!("test log line");
    }
}
