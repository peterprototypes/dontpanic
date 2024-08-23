use std::fmt::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use ring_channel::{RingReceiver, RingSender};
use tracing::{
    field::{Field, Visit},
    Event, Level, Subscriber,
};
use tracing_subscriber::{layer::Context, Layer};

use super::{send_report, Config, LogEvent, ReportLocation};

pub struct MessageVisitor<'a> {
    message: &'a mut String,
}

impl<'a> Visit for MessageVisitor<'a> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            write!(self.message, "{:?}", value).unwrap();
        } else {
            write!(self.message, "{}={:?} ", field.name(), value).unwrap();
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            write!(self.message, "{}", value).unwrap();
        } else {
            write!(self.message, "{}={:?} ", field.name(), value).unwrap();
        }
    }
}

/// A tracing [`Layer`] implementation that records tracing events.
///
/// This can be obtained via [`Client::tracing_layer`](crate::Client::tracing_layer)
pub struct TracingLayer {
    pub(crate) tx: RingSender<LogEvent>,
    pub(crate) rx: RingReceiver<LogEvent>,
    pub(crate) config: Config,
}

impl<S: Subscriber> Layer<S> for TracingLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();

        if !metadata.is_event() {
            return;
        }

        let _ = self.tx.send(LogEvent::from(event));

        if *metadata.level() != Level::ERROR || !self.config.report_on_log_errors {
            return;
        }

        let message = event_message(event);

        let loc = if let (Some(file), Some(line)) = (metadata.file(), metadata.line()) {
            Some(ReportLocation {
                file: file.to_string(),
                line,
                col: None,
            })
        } else {
            None
        };

        dbg!(&message);

        send_report(&self.config, message, loc, &self.rx)
    }
}

impl From<&Event<'_>> for LogEvent {
    fn from(event: &Event) -> Self {
        let metadata = event.metadata();

        Self {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default(),
            level: match *metadata.level() {
                Level::ERROR => 1,
                Level::WARN => 2,
                Level::INFO => 3,
                Level::DEBUG => 4,
                Level::TRACE => 5,
            },
            message: event_message(event),
            module: Some(metadata.target().to_string()),
            file: metadata.file().map(String::from),
            line: metadata.line(),
        }
    }
}

fn event_message(event: &Event<'_>) -> String {
    let metadata = event.metadata();

    let mut message = String::new();

    event.record(&mut MessageVisitor {
        message: &mut message,
    });

    if message.is_empty() {
        message = format!(
            "{} in {}: {}",
            metadata.level(),
            metadata.target(),
            metadata.name()
        );
    }

    message
}
