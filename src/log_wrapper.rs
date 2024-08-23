use std::time::{SystemTime, UNIX_EPOCH};

use log::{Level, Log, Metadata, Record};
use ring_channel::{RingReceiver, RingSender};

use super::{send_report, Config, LogEvent, ReportLocation};

impl From<&Record<'_>> for LogEvent {
    fn from(record: &Record) -> Self {
        Self {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default(),
            level: record.level() as u8,
            message: format!("{}", record.args()),
            module: record.module_path().map(String::from),
            file: record.file().map(String::from),
            line: record.line(),
        }
    }
}

pub struct LogWrapper<T> {
    pub next: T,
    pub tx: RingSender<LogEvent>,
    pub rx: RingReceiver<LogEvent>,
    pub config: Config,
}

impl<T> Log for LogWrapper<T>
where
    T: Log,
{
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.next.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        self.next.log(record);

        let _ = self.tx.send(LogEvent::from(record));

        if record.level() == Level::Error && self.config.report_on_log_errors {
            let title = format!("{}", record.args());

            let loc = if let (Some(file), Some(line)) = (record.file(), record.line()) {
                Some(ReportLocation {
                    file: file.to_string(),
                    line,
                    col: None,
                })
            } else {
                None
            };

            send_report(&self.config, title, loc, &self.rx)
        }
    }

    fn flush(&self) {}
}
