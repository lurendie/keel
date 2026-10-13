use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Local, NaiveDate};
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
const ARCHIVE_DIR_FORMAT: &str = "%m%d";
const ARCHIVE_FILE_FORMAT: &str = "%Y%m%d";

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
    root: PathBuf,
    base_path: PathBuf,
    base_name: String,
    inner: FileRotate<AppendTimestamp>,
    current_day: NaiveDate,
    written: u64,
}

impl DateRotatingWriter {
    fn new(path: PathBuf) -> Result<Self> {
        let root = path
            .parent()
            .map(Path::to_path_buf)
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        let base_name = path
            .file_name()
            .context("log path has no file name")?
            .to_string_lossy()
            .into_owned();
        let written = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        let today = Local::now().date_naive();

        let suffix =
            AppendTimestamp::with_format("%Y%m%d%H%M%S", FileLimit::Unlimited, DateFrom::Now);
        let inner = FileRotate::new(
            path.clone(),
            suffix,
            // Rotation is driven explicitly so that rotated files always land in
            // the archive folder for the day they belong to.
            ContentLimit::None,
            Compression::OnRotate(0),
            None,
        );

        let mut writer = Self {
            root,
            base_path: path,
            base_name,
            inner,
            current_day: today,
            written,
        };

        // The process may have been down over a day boundary; keep the log
        // directory limited to the current day.
        if let Some(stale_day) = writer.stale_day() {
            if let Err(err) = writer.rotate_into(stale_day) {
                eprintln!("failed to archive stale log file: {err}");
            }
        }
        prune_archives(&writer.root, today);

        Ok(writer)
    }

    /// Day the existing log file belongs to, when it is not the current one.
    fn stale_day(&self) -> Option<NaiveDate> {
        if self.written == 0 {
            return None;
        }
        let modified = fs::metadata(&self.base_path).ok()?.modified().ok()?;
        let day = DateTime::<Local>::from(modified).date_naive();
        (day != self.current_day).then_some(day)
    }

    /// Compresses the current file and moves it into the archive folder of `day`.
    fn rotate_into(&mut self, day: NaiveDate) -> io::Result<()> {
        self.inner.rotate()?;
        self.written = 0;
        if let Err(err) = self.persist_archives(day) {
            eprintln!("failed to move rotated log file into archive folder: {err}");
        }
        Ok(())
    }

    /// Moves every compressed rotation of this log into `<log dir>/<MMDD>/`.
    fn persist_archives(&self, day: NaiveDate) -> io::Result<()> {
        let prefix = format!("{}.", self.base_name);
        let folder = self.root.join(day.format(ARCHIVE_DIR_FORMAT).to_string());
        let mut moved = false;

        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }

            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.starts_with(&prefix) || !name.ends_with(".gz") {
                continue;
            }

            fs::create_dir_all(&folder)?;
            fs::rename(
                entry.path(),
                unique_archive_path(&folder, &self.base_name, day),
            )?;
            moved = true;
        }

        if moved {
            prune_archives(&self.root, Local::now().date_naive());
        }

        Ok(())
    }
}

impl Write for DateRotatingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let today = Local::now().date_naive();

        if today != self.current_day {
            let previous_day = self.current_day;
            self.current_day = today;
            // A day without log output leaves nothing to archive.
            if self.written > 0 {
                self.rotate_into(previous_day)?;
            }
        } else if self.written > 0 && self.written + buf.len() as u64 > MAX_FILE_SIZE as u64 {
            self.rotate_into(today)?;
        }

        let written = self.inner.write(buf)?;
        self.inner.flush()?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn unique_archive_path(folder: &Path, base_name: &str, day: NaiveDate) -> PathBuf {
    let stem = base_name.strip_suffix(".log").unwrap_or(base_name);
    let stamp = day.format(ARCHIVE_FILE_FORMAT);
    let mut candidate = folder.join(format!("{stem}.{stamp}.log.gz"));
    let mut index = 1;

    while candidate.exists() {
        index += 1;
        candidate = folder.join(format!("{stem}.{stamp}.{index}.log.gz"));
    }

    candidate
}

fn prune_archives(root: &Path, today: NaiveDate) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(day) = parse_month_day(name, today) else {
            continue;
        };

        if today.signed_duration_since(day).num_days() > RETENTION_DAYS {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Archive folders are named after month and day, so a folder that looks like it
/// is in the future belongs to the previous year.
fn parse_month_day(name: &str, today: NaiveDate) -> Option<NaiveDate> {
    if name.len() != 4 || !name.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    let month: u32 = name[0..2].parse().ok()?;
    let day: u32 = name[2..4].parse().ok()?;
    let candidate = NaiveDate::from_ymd_opt(today.year(), month, day)?;

    if candidate > today {
        NaiveDate::from_ymd_opt(today.year() - 1, month, day)
    } else {
        Some(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use tempfile::tempdir;

    fn root_files(directory: &Path) -> Vec<String> {
        fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter(|entry| entry.file_type().unwrap().is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()
    }

    fn folder_files(folder: &Path) -> Vec<String> {
        fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn previous_day_is_compressed_into_month_day_folder() {
        let directory = tempdir().unwrap();
        let base_path = directory.path().join("keel.info.log");
        let mut writer = DateRotatingWriter::new(base_path.clone()).unwrap();

        writer.write_all(b"first day\n").unwrap();
        writer.flush().unwrap();

        // Pretend the clock rolled over to the next day.
        writer.current_day = Local::now().date_naive() - Duration::days(1);
        let archived_day = writer.current_day;
        writer.write_all(b"second day\n").unwrap();
        writer.flush().unwrap();

        let folder = directory
            .path()
            .join(archived_day.format(ARCHIVE_DIR_FORMAT).to_string());
        let archived = folder_files(&folder);

        assert_eq!(
            archived.len(),
            1,
            "expected a single archive, got {archived:?}"
        );
        assert!(archived[0].ends_with(".gz"));
        assert!(archived[0].starts_with("keel.info."));

        // Only the current day's log file is left directly under the log directory.
        assert_eq!(root_files(directory.path()), vec!["keel.info.log"]);
        assert_eq!(fs::read(&base_path).unwrap(), b"second day\n");
    }

    #[test]
    fn multiple_rotations_in_one_day_keep_distinct_names() {
        let directory = tempdir().unwrap();
        let mut writer = DateRotatingWriter::new(directory.path().join("keel.info.log")).unwrap();
        let day = Local::now().date_naive();

        for line in [b"first\n".as_slice(), b"second\n".as_slice()] {
            writer.write_all(line).unwrap();
            writer.flush().unwrap();
            writer.rotate_into(day).unwrap();
        }

        let folder = directory
            .path()
            .join(day.format(ARCHIVE_DIR_FORMAT).to_string());
        let archived = folder_files(&folder);

        assert_eq!(archived.len(), 2, "expected two archives, got {archived:?}");
        assert_ne!(archived[0], archived[1]);
        assert!(root_files(directory.path()) == vec!["keel.info.log"]);
    }

    #[test]
    fn archives_older_than_retention_are_pruned() {
        let directory = tempdir().unwrap();
        let today = Local::now().date_naive();
        let expired = today - Duration::days(RETENTION_DAYS + 1);
        let kept = today - Duration::days(1);

        for day in [expired, kept] {
            let folder = directory
                .path()
                .join(day.format(ARCHIVE_DIR_FORMAT).to_string());
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("keel.info.20200101.log.gz"), b"x").unwrap();
        }
        fs::create_dir_all(directory.path().join("unrelated")).unwrap();

        prune_archives(directory.path(), today);

        assert!(!directory
            .path()
            .join(expired.format(ARCHIVE_DIR_FORMAT).to_string())
            .exists());
        assert!(directory
            .path()
            .join(kept.format(ARCHIVE_DIR_FORMAT).to_string())
            .exists());
        assert!(directory.path().join("unrelated").exists());
    }

    #[test]
    fn month_day_folders_resolve_across_year_boundary() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 3).unwrap();

        assert_eq!(
            parse_month_day("0103", today),
            NaiveDate::from_ymd_opt(2026, 1, 3)
        );
        assert_eq!(
            parse_month_day("1231", today),
            NaiveDate::from_ymd_opt(2025, 12, 31)
        );
        assert_eq!(parse_month_day("keel.info.log", today), None);
        assert_eq!(parse_month_day("abcd", today), None);
        assert_eq!(parse_month_day("1301", today), None);
    }
}
