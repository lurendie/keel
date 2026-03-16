use anyhow::{Context, Result};
use chrono::{Duration, Local, NaiveDate};
use file_rotate::{
    compression::Compression,
    suffix::{AppendTimestamp, DateFrom, FileLimit},
    ContentLimit, FileRotate,
};
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tracing::Level;
use tracing_subscriber::{
    filter::{EnvFilter, LevelFilter},
    fmt::{self, time::UtcTime},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    Layer, Registry,
};

const MAX_FILE_SIZE: usize = 100 * 1024 * 1024;
const RETENTION_DAYS: i64 = 7;

pub fn init() -> Result<()> {
    let log_dir = env::var("LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("logs"));
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("Failed to create log directory: {}", log_dir.display()))?;

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("keel=info,tower_http=info"));
    let console_json = env::var("LOG_FORMAT")
        .map(|value| value.eq_ignore_ascii_case("json"))
        .unwrap_or(false);

    let console_layer: Box<dyn Layer<Registry> + Send + Sync> = if console_json {
        Box::new(
            fmt::layer()
                .json()
                .with_ansi(false)
                .with_target(true)
                .with_timer(UtcTime::rfc_3339())
                .with_writer(io::stdout)
                .with_filter(env_filter),
        )
    } else {
        Box::new(
            fmt::layer()
                .pretty()
                .with_ansi(true)
                .with_target(true)
                .with_timer(UtcTime::rfc_3339())
                .with_writer(io::stdout)
                .with_filter(env_filter),
        )
    };

    let layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = vec![
        console_layer,
        level_file_layer(&log_dir, Level::ERROR)?,
        level_file_layer(&log_dir, Level::WARN)?,
        level_file_layer(&log_dir, Level::INFO)?,
        level_file_layer(&log_dir, Level::DEBUG)?,
        level_file_layer(&log_dir, Level::TRACE)?,
    ];

    tracing_subscriber::registry().with(layers).init();

    Ok(())
}

fn level_file_layer(
    log_dir: &Path,
    level: Level,
) -> Result<Box<dyn Layer<Registry> + Send + Sync>> {
    let writer = DateRotatingWriter::new(
        log_dir.join(format!("keel.{}.log", level.as_str().to_ascii_lowercase())),
    )?;
    let shared_writer = Arc::new(Mutex::new(writer));
    let layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_timer(UtcTime::rfc_3339())
        .with_writer(move || SharedWriter(shared_writer.clone()))
        .with_filter(LevelFilter::from_level(level));

    Ok(Box::new(layer))
}

struct SharedWriter(Arc<Mutex<DateRotatingWriter>>);

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("log writer lock poisoned"))?
            .write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("log writer lock poisoned"))?
            .flush()
    }
}

struct DateRotatingWriter {
    inner: FileRotate<AppendTimestamp>,
    current_day: NaiveDate,
}

impl DateRotatingWriter {
    fn new(path: PathBuf) -> Result<Self> {
        let suffix = AppendTimestamp::with_format(
            "%Y%m%d",
            FileLimit::Age(Duration::days(RETENTION_DAYS)),
            DateFrom::DateYesterday,
        );
        let inner = FileRotate::new(
            path,
            suffix,
            ContentLimit::BytesSurpassed(MAX_FILE_SIZE),
            Compression::OnRotate(0),
            None,
        );

        Ok(Self {
            inner,
            current_day: Local::now().date_naive(),
        })
    }
}

impl Write for DateRotatingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let today = Local::now().date_naive();
        if today != self.current_day {
            self.inner.rotate()?;
            self.current_day = today;
        }
        let written = self.inner.write(buf)?;
        self.inner.flush()?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rotates_and_compresses_when_day_changes() {
        let directory = tempdir().unwrap();
        let base_path = directory.path().join("test.log");
        let mut writer = DateRotatingWriter::new(base_path.clone()).unwrap();

        writer.write_all(b"first day\n").unwrap();
        writer.flush().unwrap();
        writer.current_day = Local::now().date_naive() - Duration::days(1);
        writer.write_all(b"second day\n").unwrap();
        writer.flush().unwrap();

        let files = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();

        assert!(base_path.exists());
        assert!(files.iter().any(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".gz"))
        }));
    }
}
