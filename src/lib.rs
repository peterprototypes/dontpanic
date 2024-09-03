#![cfg_attr(docsrs, feature(doc_cfg))]

//! Client library for [Don't Panic Server](https://github.com/peterprototypes/dontpanic-server)
//!
//! This crate registers a panic handler and send each panic from your application to a backend server.
//! If configured, the latest log messages before each panic are sent as well.
//! Supported logging facilities are log and tracing. By default `log::error!` and `tracing::error!` will also
//! send a report to the Don't Panic Server.
//!
//! Minimum usage example:
//!
//! ```no_run
//! use anyhow::Result;
//!
//! fn main() -> Result<()> {
//!     dontpanic::builder("<PROJECT_API_KEY>").build()?
//!
//!     // panic!
//!     Option::<u32>::None.unwrap();
//!
//!     Ok(())
//! }
//! ```
//!
//! # Using dontpanic with [log](https://docs.rs/log/latest/log/)
//!
//! Tracking down the source of a panic is easier when having the logs leading up to it.
//! Make sure to include this library with the `log` feature enabled:
//! ```toml
//! [dependencies]
//! dontpanic = { version = "*", features = ["log"] }
//! ```
//!
//! Then, [`Client::set_logger`] will accept any valid logging implementation. Initialize everything as early as possible, as panics before initialization won't be caught.
//! ```no_run
//! use anyhow::Result;
//!
//! fn main() -> Result<()> {
//!     let dontpanic = dontpanic::builder("<PROJECT_API_KEY>")
//!         .environment("production")
//!         .version(env!("CARGO_PKG_VERSION"))
//!         .build()?
//!
//!     // Important: call .build() not .init()
//!     let logger = env_logger::Builder::from_default_env().build();
//!     dontpanic.set_logger(logger)?;
//!
//!     log::info!("What's happening here?");
//!     log::error!("Booooom");
//!
//!     // panic!
//!     Option::<u32>::None.unwrap();
//!
//!     Ok(())
//! }
//! ```
//!
//! To obtain a `PROJECT_API_KEY`, check out [Don't Panic Server](https://github.com/peterprototypes/dontpanic-server) documentation.
//!
//! # Using dontpanic with [tracing](https://docs.rs/tracing/latest/)
//!
//! To enable tracing support, include dontpanic with the `tracing` feature enabled:
//! ```toml
//! [dependencies]
//! dontpanic = { version = "*", features = ["tracing"] }
//! ```
//!
//! Then `dontpanic.tracing_layer()` can be used in conjunction with any tracing subscriber. This example is with the fmt subscriber:
//! ```no_run
//! use anyhow::Result;
//!
//! use tracing_subscriber::prelude::*;
//!
//! fn main() -> Result<()> {
//!     let dontpanic = dontpanic::builder("<PROJECT_API_KEY>")
//!         .environment("production")
//!         .version(env!("CARGO_PKG_VERSION"))
//!         .build()?
//!
//!     tracing_subscriber::registry()
//!         .with(tracing_subscriber::fmt::layer())
//!         .with(dontpanic.tracing_layer())
//!         .init();
//!
//!     tracing::info!("What's happening here?");
//!     tracing::error!("Booooom");
//!     tracing::event!(Level::ERROR, the_answer = 42);
//!
//!     // panic!
//!     Option::<u32>::None.unwrap();
//!
//!     Ok(())
//! }
//! ```

use std::num::NonZeroUsize;
use std::panic;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::{backtrace::Backtrace, sync::atomic::Ordering};

#[cfg(feature = "log")]
use log::Log;
#[cfg(any(feature = "log", feature = "tracing"))]
use ring_channel::RingSender;
use ring_channel::{ring_channel, RingReceiver};
use ureq::json;

mod error;

#[cfg(feature = "tracing")]
mod tracing_layer;
#[cfg(feature = "tracing")]
pub use tracing_layer::TracingLayer;

#[cfg(feature = "log")]
mod log_wrapper;

pub use error::Error;

#[derive(Clone, Debug)]
struct Config {
    api_key: String,
    backend_url: String,
    #[cfg(any(feature = "log", feature = "tracing"))]
    report_on_log_errors: bool,
    environment: Option<String>,
    version: Option<String>,
    is_enabled: Arc<AtomicBool>,
}

/// `dontpanic` library client.
pub struct Client {
    config: Config,
    #[cfg(any(feature = "log", feature = "tracing"))]
    log_rx: RingReceiver<LogEvent>,
    #[cfg(any(feature = "log", feature = "tracing"))]
    log_tx: RingSender<LogEvent>,
}

impl Client {
    /// Sets the enabled state of the client. This is useful when using the same client instance across different environments.
    ///
    /// By default the enabled state is `true`
    ///
    /// ```no_run
    /// use anyhow::Result;
    ///
    /// fn main() -> Result<()> {
    ///     let dontpanic = dontpanic::builder("<PROJECT_API_KEY>").build()?;
    ///
    ///     // Enable in release builds only
    ///     dontpanic.set_enabled(!cfg!(debug_assertions));
    ///
    ///     Ok(())
    /// }
    ///```
    pub fn set_enabled(&self, enabled: bool) {
        self.config.is_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Register a Log implementor with this library, this sets it as the default logger. Works with any type that implements [`Log`]
    ///
    /// See [Available logging implementations](https://docs.rs/log/latest/log/#available-logging-implementations) in the [log](https://docs.rs/log/latest/log/) crate.
    ///
    /// Example with [env_logger](https://docs.rs/env_logger/latest/env_logger/):
    ///
    /// ```no_run
    /// fn main() {
    ///     let dontpanic = dontpanic::builder("<PROJECT_API_KEY>").build().unwrap()
    ///
    ///     // Important: call .build() not .init()
    ///     let logger = env_logger::Builder::from_default_env().build();
    ///     dontpanic.set_logger(logger)?;
    ///
    ///     log::info!("Luke, I am your father.");
    ///     panic!("Noooooo");
    /// }
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "log")))]
    #[cfg(feature = "log")]
    pub fn set_logger(&self, logger: impl Log + 'static) -> Result<(), Error> {
        let wrapper = log_wrapper::LogWrapper {
            next: logger,
            tx: self.log_tx.clone(),
            rx: self.log_rx.clone(),
            config: self.config.clone(),
        };

        log::set_boxed_logger(Box::new(wrapper))?;

        Ok(())
    }

    /// Creates and returns a tracing [`Layer`](tracing_subscriber::Layer) implementation.
    ///
    /// Use with [`mod@tracing_subscriber::registry`] to create a Layer stack and add this layer alongside your chosen tracing subscriber implementation.
    ///
    /// Example with [`mod@tracing_subscriber::fmt`]:
    /// ```no_run
    /// use tracing_subscriber::prelude::*;
    ///
    /// fn main() {
    ///     let dontpanic = dontpanic::builder("<PROJECT_API_KEY>").build().unwrap()
    ///
    ///     tracing_subscriber::registry()
    ///         .with(tracing_subscriber::fmt::layer())
    ///         .with(dontpanic.tracing_layer())
    ///         .init();
    ///
    ///     log::info!("Mr. Stark, I don't feel so good");
    ///     panic!("Noooooo");
    /// }
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "tracing")))]
    #[cfg(feature = "tracing")]
    pub fn tracing_layer(&self) -> TracingLayer {
        TracingLayer {
            config: self.config.clone(),
            rx: self.log_rx.clone(),
            tx: self.log_tx.clone(),
        }
    }
}

struct ReportLocation {
    file: String,
    line: u32,
    col: Option<u32>,
}

struct LogEvent {
    timestamp: u64,
    level: u8,
    message: String,
    module: Option<String>,
    file: Option<String>,
    line: Option<u32>,
}

/// A builder to configure dontpanic behavior.
///
/// Use the [builder] method in to root of this crate to create this type.
pub struct Builder {
    config: Config,
}

impl Builder {
    /// Set the reported environment. Useful for separating reports originating on dev and staging environments from production reports.
    /// You can use [debug_assertions](https://doc.rust-lang.org/cargo/reference/profiles.html#debug-assertions) to set this, or environment variables set by a CI pipeline.
    ///
    /// ```no_run
    /// use anyhow::Result;
    ///
    /// fn main() -> Result<()> {
    ///     dontpanic::builder("<PROJECT_API_KEY>")
    ///         .environment(if cfg!(debug_assertions) {
    ///             "development"
    ///         } else {
    ///             "production"
    ///         })
    ///         .build()?;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn environment(mut self, name: impl Into<String>) -> Self {
        self.config.environment = Some(name.into());
        self
    }

    /// Set your application version. Use this to track when a `panic!` or `error!` first occurred and consequently, when it is resolved.
    ///
    /// If application version tracking is done via `Cargo.toml`, the current version can be obtained via `CARGO_PKG_VERSION` env var:
    /// ```no_run
    /// fn main() -> Result<()> {
    ///     dontpanic::builder("<PROJECT_API_KEY>")
    ///         .version(env!("CARGO_PKG_VERSION"))
    ///         .build()?;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.config.version = Some(version.into());
        self
    }

    /// Controls where reports are sent to.
    ///
    /// Set this to point to the backend server url of your choice.
    /// This should be the base url of the [Don't Panic Server](https://github.com/peterprototypes/dontpanic-server) including the protocol without a
    /// trailing slash. Eg. `https://dontpanic.example.com` or `http://127.0.0.1:8080`
    ///
    /// For more information see [Don't Panic Server](https://github.com/peterprototypes/dontpanic-server) documentation.
    pub fn backend_url(mut self, url: impl AsRef<str>) -> Self {
        self.config.backend_url = format!("{}/ingress", url.as_ref());
        self
    }

    /// Enabled by default. `log::error!`, `tracing::error!` and `tracing::event!(Level::ERROR, ...` will trigger a report to be sent to the configured backend server.
    #[cfg_attr(docsrs, doc(cfg(feature = "tracing")))]
    #[cfg_attr(docsrs, doc(cfg(feature = "log")))]
    #[cfg(any(feature = "log", feature = "tracing"))]
    pub fn send_report_on_log_errors(mut self, enabled: bool) -> Self {
        self.config.report_on_log_errors = enabled;
        self
    }

    /// Builds a [`Client`] that can be used to interact with this library.
    ///
    /// This method registers a custom panic hook. The default rust hook, that prints a message to standard error and
    /// generates a backtrace is still invoked when a panic occurs.
    pub fn build(self) -> Result<Client, Error> {
        if self.config.api_key.is_empty() {
            return Err(Error::EmptyApiKey);
        }

        let (_log_tx, log_rx) = ring_channel(NonZeroUsize::try_from(100).unwrap());

        init_hook(self.config.clone(), log_rx.clone());

        Ok(Client {
            config: self.config,
            #[cfg(any(feature = "log", feature = "tracing"))]
            log_tx: _log_tx,
            #[cfg(any(feature = "log", feature = "tracing"))]
            log_rx,
        })
    }
}

/// Main entrypoint to this library. Start here and call this first. Returns a [`Builder`].
///
/// Example:
///
/// ```no_run
/// use anyhow::Result;
///
/// fn main() -> Result<()> {
///     dontpanic::builder("<PROJECT_API_KEY>").build()?;
///     panic!("This will send you an email, notification or Slack/Teams message");
/// }
/// ```
///
/// A more complete example:
///
/// ```no_run
/// use anyhow::Result;
///
/// fn main() -> Result<()> {
///     dontpanic::builder("<PROJECT_API_KEY>")
///         .environment(if cfg!(debug_assertions) {
///             "development"
///         } else {
///             "production"
///         })
///         .version(env!("CARGO_PKG_VERSION"))
///         // or
///         .version(env!("CI_COMMIT_SHORT_SHA")) // GitLab
///         .build()?;
///
///     panic!("This will send you an email, notification or Slack/Teams message");
///
///     Ok(())
/// }
/// ```
pub fn builder(api_key: impl Into<String>) -> Builder {
    let api_key = api_key.into().trim().to_string();

    Builder {
        config: Config {
            api_key,
            backend_url: "http://localhost:8080/ingress".into(),
            #[cfg(any(feature = "log", feature = "tracing"))]
            report_on_log_errors: true,
            version: None,
            environment: None,
            is_enabled: Arc::new(AtomicBool::new(true)),
        },
    }
}

fn init_hook(config: Config, log_recv: RingReceiver<LogEvent>) {
    let previous_panic_hook = panic::take_hook();

    panic::set_hook(Box::new(move |info| {
        if !config.is_enabled.load(Ordering::Relaxed) {
            previous_panic_hook(info);
            return;
        }

        let title;

        if let Some(panic_msg) = info.payload().downcast_ref::<&str>() {
            title = *panic_msg;
        } else if let Some(panic_msg) = info.payload().downcast_ref::<String>() {
            title = panic_msg;
        } else {
            previous_panic_hook(info);
            return;
        }

        let mut title = title.to_string();

        let location = info.location().map(|location| {
            title = format!("{title} in {}:{}", location.file(), location.line());

            ReportLocation {
                file: location.file().to_string(),
                line: location.line(),
                col: Some(location.column()),
            }
        });

        send_report(&config, title, location, &log_recv);

        previous_panic_hook(info);
    }));
}

fn send_report(
    config: &Config,
    title: impl Into<String>,
    loc: Option<ReportLocation>,
    log_recv: &RingReceiver<LogEvent>,
) {
    let mut log = vec![];

    while let Ok(log_event) = log_recv.try_recv() {
        log.push(json!({
            "ts": log_event.timestamp,
            "lvl": log_event.level,
            "msg": log_event.message,
            "mod": log_event.module,
            "f": log_event.file,
            "l": log_event.line,
        }));
    }

    let handle = std::thread::current();
    let backtrace = Backtrace::force_capture();

    let location = loc.map(|loc| {
        ureq::json!({
            "f": loc.file,
            "l": loc.line,
            "c": loc.col
        })
    });

    let event = ureq::json!({
        "loc": location,
        "ver": config.version,
        "tid": format!("{:?}", handle.id()),
        "tname": handle.name(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "trace": backtrace.to_string(),
        "log": log
    });

    let res = ureq::post(&config.backend_url).send_json(ureq::json!({
        "key": config.api_key,
        "env": config.environment,
        "name": title.into(),
        "data": event,
    }));

    if let Err(e) = res {
        //log::warn!(
        match e {
            ureq::Error::Status(code, response) => eprintln!(
                "Error sending report to {}. Code: {}, Response: {:?}",
                config.backend_url,
                code,
                response.into_string()
            ),
            ureq::Error::Transport(e) => eprintln!(
                "Transport error sending report to {}. Error: {:?}",
                config.backend_url, e
            ),
        };
    }
}
