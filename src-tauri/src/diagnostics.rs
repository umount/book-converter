//! Local diagnostic logs. No provider payloads, credentials or project databases.
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub const SETTING: &str = "full_logging";
type Reload = tracing_subscriber::reload::Handle<EnvFilter, tracing_subscriber::Registry>;
static FILTER: OnceLock<Reload> = OnceLock::new();
const MAX_BYTES: u64 = 20 * 1024 * 1024;
const KEEP_FILES: usize = 5;

pub fn directory() -> PathBuf {
    crate::paths::app_data_dir().join("logs")
}
pub fn enabled() -> bool {
    crate::settings::get(&crate::settings::db_path(), SETTING)
        .ok()
        .flatten()
        .map(|value| value == "true")
        .unwrap_or(cfg!(feature = "diagnostics"))
}
fn filter(full: bool) -> EnvFilter {
    // Do not enable HTTP dependency traces: they can contain headers or bodies.
    EnvFilter::new(if full {
        "warn,book_converter_lib=trace,manga_inference=debug"
    } else {
        "warn,book_converter_lib=info,manga_inference=warn"
    })
}
pub fn configure(full: bool) {
    if let Some(handle) = FILTER.get() {
        let _ = handle.reload(filter(full));
    }
    tracing::info!(
        full_logging = full,
        "Diagnostic logging configuration changed"
    );
}

struct LogFile {
    file: File,
    size: u64,
    path: PathBuf,
}
impl LogFile {
    fn open(path: PathBuf) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        Ok(Self { file, size, path })
    }
}
impl Write for LogFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.size + bytes.len() as u64 > MAX_BYTES {
            self.file.flush()?;
            // Copy then truncate works on Windows with the active file open.
            std::fs::copy(&self.path, self.path.with_extension("previous.log"))?;
            self.file.set_len(0)?;
            self.size = 0;
        }
        self.file.write_all(bytes)?;
        self.size += bytes.len() as u64;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
fn log_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry.file_name().to_string_lossy().starts_with("session-")
            && entry.path().extension().is_some_and(|ext| ext == "log")
        {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}
pub fn init() {
    let full = enabled();
    let (layer, handle) = tracing_subscriber::reload::Layer::new(filter(full));
    let _ = FILTER.set(handle);
    let writer = (|| -> io::Result<_> {
        let dir = directory();
        std::fs::create_dir_all(&dir)?;
        let files = log_files(&dir)?;
        for path in files
            .iter()
            .take(files.len().saturating_sub(KEEP_FILES - 1))
        {
            std::fs::remove_file(path)?;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        LogFile::open(dir.join(format!("session-{stamp}-{}.log", std::process::id())))
    })();
    match writer {
        Ok(file) => {
            tracing_subscriber::registry()
                .with(layer)
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .with_writer(Mutex::new(file)),
                )
                .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
                .init();
        }
        Err(error) => {
            tracing_subscriber::registry()
                .with(layer)
                .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
                .init();
            tracing::error!(%error, "Cannot create diagnostic log file");
        }
    }
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(location = ?info.location(), "Application panic");
        tracing::debug!(backtrace = %std::backtrace::Backtrace::force_capture(), "Panic backtrace");
        previous_hook(info);
    }));
    tracing::info!(version = env!("CARGO_PKG_VERSION"), commit = env!("BC_COMMIT"),
        os = std::env::consts::OS, arch = std::env::consts::ARCH,
        debug = cfg!(debug_assertions), full_logging = full,
        logs = %directory().display(), "Application starting");
}

/// Archive only our bounded logs and build metadata, never settings or API keys.
pub fn export(destination: &Path) -> anyhow::Result<()> {
    export_from(&directory(), destination)
}
fn export_from(dir: &Path, destination: &Path) -> anyhow::Result<()> {
    let files = log_files(dir)?;
    anyhow::ensure!(!files.is_empty(), "diagnostic_logs_missing");
    let mut zip = zip::ZipWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?,
    );
    let result = (|| -> anyhow::Result<()> {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("build.txt", options)?;
        writeln!(
            zip,
            "version={}\ncommit={}\nos={}\narch={}\ndebug={}\nfull_logging={}",
            env!("CARGO_PKG_VERSION"),
            env!("BC_COMMIT"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            cfg!(debug_assertions),
            enabled()
        )?;
        for path in files.iter().rev().take(KEEP_FILES) {
            zip.start_file(path.file_name().unwrap().to_string_lossy(), options)?;
            io::copy(
                &mut std::io::Read::take(File::open(path)?, MAX_BYTES),
                &mut zip,
            )?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        drop(zip);
        let _ = std::fs::remove_file(destination);
        return Err(error);
    }
    zip.finish()?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logging_can_be_enabled_without_restarting_or_logging_http_payloads() {
        let path =
            std::env::temp_dir().join(format!("diagnostics-filter-{}.log", uuid::Uuid::new_v4()));
        let writer = LogFile::open(path.clone()).unwrap();
        let (layer, handle) = tracing_subscriber::reload::Layer::new(filter(false));
        let subscriber = tracing_subscriber::registry().with(layer).with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(Mutex::new(writer)),
        );
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(target: "book_converter_lib", "hidden_before");
            tracing::info!(target: "book_converter_lib", "visible_info");
            handle.reload(filter(true)).unwrap();
            tracing::debug!(target: "book_converter_lib", "visible_after");
            tracing::trace!(target: "reqwest", "hidden_http");
            handle.reload(filter(false)).unwrap();
            tracing::debug!(target: "book_converter_lib", "hidden_after");
        });
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("visible_info") && text.contains("visible_after"));
        assert!(
            !text.contains("hidden_before")
                && !text.contains("hidden_http")
                && !text.contains("hidden_after")
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn archive_contains_only_logs_and_does_not_overwrite_existing_files() {
        let dir = std::env::temp_dir().join(format!("diagnostics-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("session-001.log"), b"runtime missing").unwrap();
        std::fs::write(dir.join("settings.db"), b"secret").unwrap();
        let destination = dir.join("report.zip");
        export_from(&dir, &destination).unwrap();
        let bytes = std::fs::read(&destination).unwrap();
        assert!(export_from(&dir, &destination).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(zip.len(), 2);
        assert!(zip.by_name("build.txt").is_ok());
        assert!(zip.by_name("session-001.log").is_ok());
        assert!(zip.by_name("settings.db").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rotation_retains_recent_data_and_bounds_active_file() {
        let dir = std::env::temp_dir().join(format!("diagnostics-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("session-test.log");
        let mut writer = LogFile::open(path.clone()).unwrap();
        writer.write_all(b"old").unwrap();
        writer.size = MAX_BYTES;
        writer.write_all(b"new").unwrap();
        writer.flush().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(
            std::fs::read(path.with_extension("previous.log")).unwrap(),
            b"old"
        );
        drop(writer);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
