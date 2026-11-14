//! The platform's words on stderr: GPUI says through `log` why something the app asked of the system did not happen
//! (a notification not allowed, or not sent outside a bundle). Warnings and errors are printed, and what the
//! notification centre says at any level, so the bundle's log tells whether notifications go out.

struct Stderr;

impl log::Log for Stderr {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        m.level() <= log::Level::Warn || m.target().contains("notification")
    }

    fn log(&self, r: &log::Record<'_>) {
        if self.enabled(r.metadata()) {
            eprintln!("doze-app: {} {}: {}", r.level(), r.target(), r.args());
        }
    }

    fn flush(&self) {}
}

static LOGGER: Stderr = Stderr;

/// Puts the logger in place, once, first thing.
pub(crate) fn init() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}
