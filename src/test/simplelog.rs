struct SimpleLogger;

use once_cell::sync::Lazy;

static TEST_LOG_ENABLED: Lazy<bool> = Lazy::new(|| {
    std::env::var("TEST_LOG")
        .map(|v| v != "0" && v != "false")
        .unwrap_or(false)
});

impl ::log::Log for SimpleLogger {
    fn enabled(&self, metadata: &::log::Metadata) -> bool {
        metadata.target().starts_with("hreq")
    }

    fn log(&self, record: &::log::Record) {
        if *TEST_LOG_ENABLED && self.enabled(record.metadata()) {
            println!("{} {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

use ::log::LevelFilter;

static LOGGER: SimpleLogger = SimpleLogger;

pub fn set_logger() {
    static INIT: ::std::sync::Once = ::std::sync::Once::new();
    INIT.call_once(|| {
        ::log::set_logger(&LOGGER)
            .map(|()| ::log::set_max_level(LevelFilter::Trace))
            .expect("Failed to set logger")
    });
}
