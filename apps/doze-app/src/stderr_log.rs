//! The platform's words on stderr: GPUI says through `log` why something the app asked of the system did not happen
//! (a notification not allowed, or not sent outside a bundle). Warnings and errors are printed, and what the
//! notification centre says at any level, so the bundle's log tells whether notifications go out.

/// `all`: `DOZE_APP_LOG=info` (or `debug`) prints every line at that level, for looking into the platform.
struct Stderr {
    all: Option<log::Level>,
}

impl log::Log for Stderr {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        m.level() <= log::Level::Warn
            || m.target().contains("notification")
            || self.all.is_some_and(|l| m.level() <= l)
    }

    fn log(&self, r: &log::Record<'_>) {
        if self.enabled(r.metadata()) {
            eprintln!("doze-app: {} {}: {}", r.level(), r.target(), r.args());
        }
    }

    fn flush(&self) {}
}

/// Puts the logger in place, once, first thing.
pub(crate) fn init() {
    let all = std::env::var("DOZE_APP_LOG")
        .ok()
        .and_then(|l| l.parse::<log::Level>().ok());
    let max = all.map_or(log::LevelFilter::Info, |l| {
        l.to_level_filter().max(log::LevelFilter::Info)
    });
    if log::set_boxed_logger(Box::new(Stderr { all })).is_ok() {
        log::set_max_level(max);
    }
}
