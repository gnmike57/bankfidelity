use crate::app::config::AppConfig;
#[cfg(feature = "otel")]
use opentelemetry_otlp::WithExportConfig;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::{Duration, SystemTime};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

use std::io::{Read, Seek, SeekFrom, Write};
use tracing_subscriber::fmt::MakeWriter;

pub struct ScrubbingWriter<W> {
    inner: W,
}

impl<W: Write> ScrubbingWriter<W> {
    pub(crate) fn scrub(text: &str) -> String {
        static RE_EMAIL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        static RE_KEYS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        static RE_MAC_PATH: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        static RE_WIN_PATH: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

        let re_email = RE_EMAIL.get_or_init(|| {
            regex::Regex::new(r"[a-zA-Z0-9_.+-]+@[a-zA-Z0-9-]+\.[a-zA-Z0-9-.]+").unwrap()
        });
        let re_keys = RE_KEYS.get_or_init(|| regex::Regex::new(r#"(?i)(api[_-]?key|token|secret|password|bearer)[\s=:>]+['"]?[A-Za-z0-9\-_]{16,}['"]?"#).unwrap());
        let re_mac_path =
            RE_MAC_PATH.get_or_init(|| regex::Regex::new(r"/Users/[a-zA-Z0-9_.-]+").unwrap());
        let re_win_path =
            RE_WIN_PATH.get_or_init(|| regex::Regex::new(r"C:\\Users\\[a-zA-Z0-9_.-]+").unwrap());

        let text = re_email.replace_all(text, "***@***.***");
        let text = re_keys.replace_all(&text, "${1}=***");
        let text = re_mac_path.replace_all(&text, "~");
        let text = re_win_path.replace_all(&text, r"C:\Users\~");

        text.to_string()
    }
}

impl<W: Write> Write for ScrubbingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(s) = std::str::from_utf8(buf) {
            let scrubbed = Self::scrub(s);
            self.inner.write_all(scrubbed.as_bytes())?;
            Ok(buf.len())
        } else {
            self.inner.write(buf)
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub struct ScrubbingMakeWriter<M> {
    inner: M,
}

impl<M> ScrubbingMakeWriter<M> {
    pub fn new(inner: M) -> Self {
        Self { inner }
    }
}

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for ScrubbingMakeWriter<M> {
    type Writer = ScrubbingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        ScrubbingWriter {
            inner: self.inner.make_writer(),
        }
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Self::Writer {
        ScrubbingWriter {
            inner: self.inner.make_writer_for(meta),
        }
    }
}

const MANAGED_LOG_PREFIXES: [&str; 2] = ["app.log", "error.log"];
const DEFAULT_LOG_RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const DEFAULT_MAX_FILES_PER_STREAM: usize = 28;

fn managed_log_files(log_dir: &Path, prefix: &str) -> std::io::Result<Vec<(PathBuf, SystemTime)>> {
    let mut files = Vec::new();
    if !log_dir.exists() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(prefix) {
            continue;
        }
        let modified = entry
            .metadata()?
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        files.push((entry.path(), modified));
    }
    files.sort_by(|left, right| right.1.cmp(&left.1));
    Ok(files)
}

pub fn enforce_log_retention(
    log_dir: &Path,
    max_age: Duration,
    max_files_per_stream: usize,
) -> std::io::Result<usize> {
    std::fs::create_dir_all(log_dir)?;
    let now = SystemTime::now();
    let mut removed = 0usize;
    for prefix in MANAGED_LOG_PREFIXES {
        for (index, (path, modified)) in managed_log_files(log_dir, prefix)?.into_iter().enumerate()
        {
            let expired = now.duration_since(modified).unwrap_or_default() > max_age;
            if expired || index >= max_files_per_stream {
                std::fs::remove_file(path)?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

pub fn support_log_tail(
    log_dir: &Path,
    max_lines: usize,
    max_bytes: usize,
) -> std::io::Result<String> {
    let Some((path, _)) = managed_log_files(log_dir, "app.log")?.into_iter().next() else {
        return Ok(String::new());
    };
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(max_bytes as u64);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((length - start) as usize);
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let tail = text
        .lines()
        .rev()
        .take(max_lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ScrubbingWriter::<std::io::Sink>::scrub(&tail))
}

static PANIC_HOOK: Once = Once::new();

pub struct TelemetryGuard {
    // Hold onto the guard for the non-blocking file appender if we ever use one
    // But tracing_appender::rolling doesn't strictly need a guard to stay alive
    // unlike the non_blocking one, however we return a guard for OTLP flush
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        #[cfg(feature = "otel")]
        opentelemetry::global::shutdown_tracer_provider();
    }
}

fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let payload = info
                .payload()
                .downcast_ref::<&'static str>()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<unknown panic payload>".to_string());
            let location = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "<unknown location>".to_string());
            tracing::error!("[PANIC] at {} -- {}", location, payload);
            default_hook(info);
        }));
    });
}

pub fn init(cfg: &AppConfig) -> TelemetryGuard {
    // Best-effort log directory creation. Config loading already validates
    // this, but we re-check here and warn clearly if it regressed (e.g. the
    // directory was removed between config load and telemetry init).
    if let Err(e) = std::fs::create_dir_all(&cfg.log_dir) {
        eprintln!(
            "⚠️ Could not create log directory '{}': {}. File logging will be disabled; \
             logs will go to stdout only.",
            cfg.log_dir.display(),
            e
        );
    } else if let Err(error) = enforce_log_retention(
        &cfg.log_dir,
        DEFAULT_LOG_RETENTION,
        DEFAULT_MAX_FILES_PER_STREAM,
    ) {
        eprintln!(
            "⚠️ Could not enforce log retention in '{}': {}.",
            cfg.log_dir.display(),
            error
        );
    }
    install_panic_hook();

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let stdout_appender = ScrubbingMakeWriter::new(std::io::stdout);
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(stdout_appender)
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true);

    let file_appender = tracing_appender::rolling::daily(&cfg.log_dir, "app.log");
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(ScrubbingMakeWriter::new(file_appender))
        .with_ansi(false)
        .with_thread_ids(true)
        .with_thread_names(true);

    let error_appender = tracing_appender::rolling::daily(&cfg.log_dir, "error.log");
    let error_layer = tracing_subscriber::fmt::layer()
        .with_writer(ScrubbingMakeWriter::new(error_appender))
        .with_ansi(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_filter(EnvFilter::new("warn"));

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .with(error_layer);

    #[cfg(feature = "otel")]
    if let Some(endpoint) = &cfg.otel_endpoint {
        // OTLP requires a running tokio runtime - gracefully degrade if absent.
        let in_tokio = tokio::runtime::Handle::try_current().is_ok();
        if !in_tokio {
            eprintln!(
                "⚠️ OTLP endpoint '{endpoint}' configured but no Tokio runtime is available; skipping OTLP install."
            );
            subscriber.init();
            return TelemetryGuard {};
        }

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint)
                .build()?;

            let resource = opentelemetry_sdk::Resource::new(vec![opentelemetry::KeyValue::new(
                "service.name",
                cfg.otel_service_name.clone(),
            )]);

            let provider = opentelemetry_sdk::trace::TracerProvider::builder()
                .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
                .with_resource(resource)
                .build();

            use opentelemetry::trace::TracerProvider;
            let tracer = provider.tracer("dual-core-pdf-pipeline");
            opentelemetry::global::set_tracer_provider(provider);
            Ok::<_, opentelemetry::trace::TraceError>(tracer)
        }));

        match result {
            Ok(Ok(tracer)) => {
                let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
                subscriber.with(otel_layer).init();
            }
            Ok(Err(e)) => {
                eprintln!(
                    "⚠️ Failed to initialize OTLP tracer at {endpoint}: {e}. Continuing without OTLP."
                );
                subscriber.init();
            }
            Err(_) => {
                eprintln!(
                    "⚠️ OTLP install panicked (likely missing runtime); continuing without OTLP."
                );
                subscriber.init();
            }
        }
    } else {
        subscriber.init();
    }

    // Recommendation #8: when built without the `otel` feature, there is no
    // exporter to install - just bring up stdout/file logging.
    #[cfg(not(feature = "otel"))]
    subscriber.init();

    TelemetryGuard {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "otel")]
    use std::path::PathBuf;

    #[cfg(feature = "otel")]
    #[test]
    fn init_with_unreachable_otlp_endpoint_does_not_panic() {
        let mut cfg = AppConfig::default();
        cfg.log_dir = PathBuf::from("logs_test");
        cfg.otel_endpoint = Some("http://127.0.0.1:1".into());
        cfg.otel_service_name = "test-service".into();

        // Should not panic
        let _guard = init(&cfg);
    }

    #[test]
    fn test_scrubbing_writer() {
        let mut buf = Vec::new();
        let mut writer = ScrubbingWriter { inner: &mut buf };

        let msg = "Hello user@example.com with API_KEY=1234567890abcdef123 and path /Users/test_user/file.txt and C:\\Users\\test_user\\file.txt";
        writer.write_all(msg.as_bytes()).unwrap();
        writer.flush().unwrap();

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("***@***.***"));
        assert!(output.contains("API_KEY=***"));
        assert!(output.contains("~"));
        assert!(output.contains("C:\\Users\\~"));
        assert!(!output.contains("user@example.com"));
        assert!(!output.contains("1234567890abcdef123"));
        assert!(!output.contains("/Users/test_user/file.txt"));
    }

    #[test]
    fn retention_bounds_each_managed_log_stream() {
        let temp = tempfile::tempdir().unwrap();
        for prefix in MANAGED_LOG_PREFIXES {
            for index in 0..5 {
                std::fs::write(
                    temp.path().join(format!("{prefix}.{index}")),
                    format!("entry {index}"),
                )
                .unwrap();
            }
        }
        std::fs::write(temp.path().join("unmanaged.txt"), "keep").unwrap();

        let removed =
            enforce_log_retention(temp.path(), Duration::from_secs(24 * 60 * 60), 2).unwrap();
        assert_eq!(removed, 6);
        for prefix in MANAGED_LOG_PREFIXES {
            assert_eq!(managed_log_files(temp.path(), prefix).unwrap().len(), 2);
        }
        assert!(temp.path().join("unmanaged.txt").exists());
    }

    #[test]
    fn support_tail_is_bounded_and_rescrubbed() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("app.log.2026-08-01"),
            "first\nuser@example.com API_KEY=1234567890abcdef123 /Users/private/file.pdf\nlast\n",
        )
        .unwrap();

        let tail = support_log_tail(temp.path(), 2, 4096).unwrap();
        assert!(tail.contains("last"));
        assert!(tail.contains("***@***.***"));
        assert!(tail.contains("API_KEY=***"));
        assert!(!tail.contains("user@example.com"));
        assert!(!tail.contains("1234567890abcdef123"));
        assert!(!tail.contains("/Users/private"));
        assert!(!tail.contains("first"));
    }
}
