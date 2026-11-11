//! The logging of the command lines (`fontTools.misc.loggingTools.configLogger`
//! with its default formats): messages go to standard error, with the level
//! name except for INFO and DEBUG messages.

use std::io::Write;

/// A logger that writes to standard error in fontTools' default format.
#[derive(Debug, Clone, Copy, Default)]
pub struct StderrLogger;

impl log::Log for StderrLogger {
	fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
		metadata.level() <= log::max_level()
	}

	fn log(&self, record: &log::Record<'_>) {
		if !self.enabled(record.metadata()) {
			return;
		}
		let message = format(record.level(), &record.args().to_string());
		// Python's standard error translates `\n` to `\r\n` on Windows.
		let message = if cfg!(windows) {
			message.replace('\n', "\r\n")
		} else {
			message
		};
		let _ = std::io::stderr().write_all(message.as_bytes());
	}

	fn flush(&self) {
		let _ = std::io::stderr().flush();
	}
}

/// A message as `DEFAULT_FORMATS` formats it, with the line ending.
pub fn format(level: log::Level, message: &str) -> String {
	let name = match level {
		log::Level::Error => "ERROR",
		log::Level::Warn => "WARNING",
		log::Level::Info | log::Level::Debug | log::Level::Trace => {
			return format!("{message}\n");
		}
	};
	format!("{name}: {message}\n")
}

static LOGGER: StderrLogger = StderrLogger;

/// Installs [`StderrLogger`] at level INFO (`configLogger(level="INFO")`), unless
/// a logger is installed already.
pub fn config_logger() {
	if log::set_logger(&LOGGER).is_ok() {
		log::set_max_level(log::LevelFilter::Info);
	}
}
