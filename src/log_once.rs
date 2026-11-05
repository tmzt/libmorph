//! `log_once!`: say something at warn the first time a call site is reached.

/// Log at warn the first time **this call site** is reached, and never
/// again. One `AtomicBool` per expansion, so two sites speak
/// independently and one site speaks once however many frames pass
/// through it.
///
/// Internal, and for one job: the default body of a hook a contract
/// requires, where a silent no-op would leave a black screen with nothing
/// to read. A default that is a real choice - `limits`, `wgpu_config`,
/// every `on_*` - stays silent.
///
/// A `static` inside a generic body is one item, not one per
/// instantiation, so the trait's default speaks once for the process
/// rather than once per app type. That is what is wanted: the message
/// names the mistake, and repeating it per type would not.
macro_rules! log_once {
    ($($arg:tt)*) => {{
        static LOGGED: ::std::sync::atomic::AtomicBool =
            ::std::sync::atomic::AtomicBool::new(false);
        if !LOGGED.swap(true, ::std::sync::atomic::Ordering::Relaxed) {
            ::log::warn!($($arg)*);
        }
    }};
}

pub(crate) use log_once;

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    /// Keeps the lines it is given, so a test can count what one site
    /// produced. Nothing else in the crate installs a logger, and the
    /// lines are matched by a marker, so other tests logging in parallel
    /// cannot change the count.
    struct Collect;

    static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

    impl log::Log for Collect {
        fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
            true
        }

        fn log(&self, record: &log::Record<'_>) {
            LINES.lock().unwrap().push(record.args().to_string());
        }

        fn flush(&self) {}
    }

    /// How many kept lines contain `marker`.
    fn kept(marker: &str) -> usize {
        LINES
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.contains(marker))
            .count()
    }

    #[test]
    fn one_line_per_site_however_often_it_is_reached() {
        // `set_logger` takes once per process; if something else has it,
        // there is nothing to count and the macro is untestable here.
        if log::set_logger(&Collect).is_err() {
            return;
        }
        log::set_max_level(log::LevelFilter::Warn);

        for _ in 0..3 {
            log_once!("log_once: first site");
        }
        for _ in 0..3 {
            log_once!("log_once: second site {}", 7);
        }
        assert_eq!(kept("log_once: first site"), 1, "one line for the site");
        assert_eq!(kept("log_once: second site 7"), 1, "and one for the other");
    }
}
