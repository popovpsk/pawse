use std::backtrace::{Backtrace, BacktraceStatus};
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, SystemTime};

use log::{LevelFilter, Log, Metadata, Record};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct Notice {
    pub severity: Severity,
    pub title: String,
    pub message: String,
}

pub struct Config {
    pub log_dir: PathBuf,
    pub level: LevelFilter,
    pub also_stderr: bool,
    pub max_bytes: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_dir: PathBuf::from("."),
            level: LevelFilter::Info,
            also_stderr: cfg!(debug_assertions),
            max_bytes: 5 * 1024 * 1024,
        }
    }
}

static NOTICE_TX: OnceLock<flume::Sender<Notice>> = OnceLock::new();
static LINE_TX: OnceLock<flume::Sender<String>> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(config: Config) -> flume::Receiver<Notice> {
    let path = config.log_dir.join("pawse.log");
    let _ = LOG_PATH.set(path.clone());

    let (line_tx, line_rx) = flume::unbounded::<String>();
    let _ = LINE_TX.set(line_tx.clone());
    spawn_writer(path, config.max_bytes, config.also_stderr, line_rx);

    let logger = FileLogger {
        tx: line_tx,
        level: config.level,
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(config.level);
    }

    install_panic_hook();

    let (notice_tx, notice_rx) = flume::unbounded::<Notice>();
    let _ = NOTICE_TX.set(notice_tx);
    notice_rx
}

pub fn flush() {
    let Some(tx) = LINE_TX.get() else {
        return;
    };
    let mut waited = false;
    for _ in 0..40 {
        if tx.is_empty() {
            break;
        }
        waited = true;
        thread::sleep(Duration::from_millis(5));
    }
    if waited {
        thread::sleep(Duration::from_millis(5));
    }
}

pub fn notify_error(title: impl Into<String>, message: impl Into<String>) {
    push_notice(Severity::Error, title.into(), message.into());
}

pub fn notify_warning(title: impl Into<String>, message: impl Into<String>) {
    push_notice(Severity::Warning, title.into(), message.into());
}

fn push_notice(severity: Severity, title: String, message: String) {
    match severity {
        Severity::Error => log::error!("{title}: {message}"),
        Severity::Warning => log::warn!("{title}: {message}"),
    }
    if let Some(tx) = NOTICE_TX.get() {
        let _ = tx.send(Notice {
            severity,
            title,
            message,
        });
    }
}

struct FileLogger {
    tx: flume::Sender<String>,
    level: LevelFilter,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}: {}",
            humantime::format_rfc3339_millis(SystemTime::now()),
            record.level(),
            record.target(),
            record.args()
        );
        let _ = self.tx.send(line);
    }

    fn flush(&self) {}
}

fn spawn_writer(path: PathBuf, max_bytes: u64, also_stderr: bool, rx: flume::Receiver<String>) {
    thread::Builder::new()
        .name("pawse-diagnostics".to_string())
        .spawn(move || {
            let mut writer = open_log(&path);
            let mut size = current_size(&path);
            while let Ok(line) = rx.recv() {
                write_line(&mut writer, &mut size, &path, max_bytes, also_stderr, &line);
            }
        })
        .ok();
}

fn write_line(
    writer: &mut Option<BufWriter<File>>,
    size: &mut u64,
    path: &Path,
    max_bytes: u64,
    also_stderr: bool,
    line: &str,
) {
    if also_stderr {
        let mut err = std::io::stderr();
        let _ = writeln!(err, "{line}");
    }
    if let Some(w) = writer.as_mut()
        && writeln!(w, "{line}").is_ok()
    {
        let _ = w.flush();
        *size += line.len() as u64 + 1;
    }
    if *size >= max_bytes {
        *writer = None;
        rotate(path);
        *writer = open_log(path);
        *size = 0;
    }
}

fn open_log(path: &Path) -> Option<BufWriter<File>> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()
        .map(BufWriter::new)
}

fn rotate(path: &Path) {
    let _ = fs::rename(path, path.with_extension("log.1"));
}

fn current_size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_sync(&format_panic(info));
        previous(info);
    }));
}

fn format_panic(info: &PanicHookInfo) -> String {
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "unknown".to_string());
    let payload = panic_payload(info);
    let ts = humantime::format_rfc3339_millis(SystemTime::now());
    let backtrace = Backtrace::capture();
    if backtrace.status() == BacktraceStatus::Captured {
        format!("{ts} ERROR panic: {location}: {payload}\n{backtrace}")
    } else {
        format!("{ts} ERROR panic: {location}: {payload}")
    }
}

fn panic_payload(info: &PanicHookInfo) -> String {
    let payload = info.payload();
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "Box<dyn Any>".to_string()
    }
}

fn write_sync(line: &str) {
    if let Some(path) = LOG_PATH.get() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }
    let mut err = std::io::stderr();
    let _ = writeln!(err, "{line}");
}

const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<FileLogger>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::time::UNIX_EPOCH;

    fn unique_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pawse-diag-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(path: &Path) -> String {
        let mut s = String::new();
        File::open(path).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn writes_line_to_file() {
        let path = unique_dir().join("pawse.log");
        let mut writer = open_log(&path);
        let mut size = current_size(&path);
        write_line(
            &mut writer,
            &mut size,
            &path,
            1024,
            false,
            "hello diagnostics",
        );
        drop(writer);
        assert!(read(&path).contains("hello diagnostics"));
    }

    #[test]
    fn rotates_past_max_bytes() {
        let path = unique_dir().join("pawse.log");
        let mut writer = open_log(&path);
        let mut size = current_size(&path);
        for i in 0..50 {
            write_line(
                &mut writer,
                &mut size,
                &path,
                64,
                false,
                &format!("line {i} with some padding to grow the file"),
            );
        }
        drop(writer);
        assert!(path.with_extension("log.1").exists());
    }
}
