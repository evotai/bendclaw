use std::fs::File;
use std::fs::OpenOptions;
use std::fs::{self};
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use chrono::Local;
use parking_lot::Mutex;
use tracing_subscriber::registry::LookupSpan;

/// An hourly-rolling file writer that uses local time for boundaries.
///
/// `tracing_appender::rolling::daily` uses UTC, which causes the log filename
/// to lag behind in positive-offset timezones (e.g. UTC+8). This writer rolls
/// based on `chrono::Local` so the filename matches the operator's wall clock.
/// Old log files beyond `max_files` are automatically cleaned up.
pub struct LocalDailyWriter {
    dir: PathBuf,
    prefix: String,
    max_files: usize,
    state: Mutex<WriterState>,
}

struct WriterState {
    current_hour: String,
    file: File,
}

impl LocalDailyWriter {
    pub fn new(dir: impl AsRef<Path>, prefix: impl Into<String>) -> std::io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let prefix = prefix.into();
        let hour = Local::now().format("%Y-%m-%d-%H").to_string();
        let file = open_log_file(&dir, &prefix, &hour)?;
        let writer = Self {
            dir,
            prefix,
            max_files: 5,
            state: Mutex::new(WriterState {
                current_hour: hour,
                file,
            }),
        };
        writer.cleanup_old_files();
        Ok(writer)
    }

    fn cleanup_old_files(&self) {
        // Collect date subdirectories (e.g. "2026-03-20"), sorted ascending.
        let mut date_dirs: Vec<_> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        date_dirs.sort_by_key(|e| e.file_name());
        if date_dirs.len() > self.max_files {
            let to_remove = date_dirs.len() - self.max_files;
            for entry in date_dirs.into_iter().take(to_remove) {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

fn open_log_file(dir: &Path, prefix: &str, hour: &str) -> std::io::Result<File> {
    // hour format: "2026-03-20-08" → date subdir: "2026-03-20"
    let date = hour.rsplitn(2, '-').last().unwrap_or(hour);
    let subdir = dir.join(date);
    fs::create_dir_all(&subdir)?;
    let path = subdir.join(format!("{prefix}.{hour}"));
    OpenOptions::new().create(true).append(true).open(path)
}

impl Write for &LocalDailyWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let now_hour = Local::now().format("%Y-%m-%d-%H").to_string();
        let mut state = self.state.lock();
        if state.current_hour != now_hour {
            if let Ok(f) = open_log_file(&self.dir, &self.prefix, &now_hour) {
                state.file = f;
                state.current_hour = now_hour;
                drop(state);
                self.cleanup_old_files();
                return self.state.lock().file.write(buf);
            }
        }
        state.file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.state.lock().file.flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LocalDailyWriter {
    type Writer = &'a Self;

    fn make_writer(&'a self) -> Self::Writer {
        self
    }
}

pub struct TargetFirstFormatter;

/// Fields hidden from human-readable text output (still present in JSON).
const HIDDEN_FIELDS: &[&str] = &[
    "stage",
    "status",
    "session_id",
    "agent_id",
    "tool_call_id",
    "tool_kind",
    "operation",
];

/// A tracing field visitor that collects the message and visible fields,
/// skipping fields in `HIDDEN_FIELDS` and zero-value numerics.
struct HumanFieldVisitor {
    message: String,
    fields: String,
}

impl HumanFieldVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
            fields: String::new(),
        }
    }

    fn is_hidden(field: &str) -> bool {
        HIDDEN_FIELDS.contains(&field)
    }
}

impl tracing::field::Visit for HumanFieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            use std::fmt::Write;
            write!(self.message, "{value:?}").ok();
        } else if !Self::is_hidden(field.name()) {
            use std::fmt::Write;
            write!(self.fields, " {}={:?}", field.name(), value).ok();
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else if !Self::is_hidden(field.name()) {
            use std::fmt::Write;
            if value.contains(' ') {
                write!(self.fields, " {}=\"{}\"", field.name(), value).ok();
            } else {
                write!(self.fields, " {}={}", field.name(), value).ok();
            }
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if Self::is_hidden(field.name()) || value == 0 {
            return;
        }
        use std::fmt::Write;
        write!(self.fields, " {}={}", field.name(), value).ok();
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        if Self::is_hidden(field.name()) || value == 0 {
            return;
        }
        use std::fmt::Write;
        write!(self.fields, " {}={}", field.name(), value).ok();
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        if Self::is_hidden(field.name()) || value == 0.0 {
            return;
        }
        use std::fmt::Write;
        write!(self.fields, " {}={}", field.name(), value).ok();
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if Self::is_hidden(field.name()) {
            return;
        }
        use std::fmt::Write;
        write!(self.fields, " {}={}", field.name(), value).ok();
    }
}

impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for TargetFirstFormatter
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let ansi = writer.has_ansi_escapes();
        let level = *event.metadata().level();

        // Timestamp — dim gray
        if ansi {
            write!(
                writer,
                "\x1b[2m{}\x1b[0m",
                Local::now().format("%H:%M:%S%.3f")
            )?;
        } else {
            write!(writer, "{}", Local::now().format("%Y-%m-%dT%H:%M:%S%.3f"))?;
        }

        // Level — colored by severity
        if ansi {
            let color = match level {
                tracing::Level::ERROR => "\x1b[1;31m",
                tracing::Level::WARN => "\x1b[1;33m",
                tracing::Level::INFO => "\x1b[1;32m",
                tracing::Level::DEBUG => "\x1b[1;34m",
                tracing::Level::TRACE => "\x1b[35m",
            };
            write!(writer, " {color}{level:>5}\x1b[0m")?;
        } else {
            write!(writer, " {level:>5}")?;
        }

        // Spans — yellow
        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                if ansi {
                    write!(writer, " \x1b[33m{}{{", span.name())?;
                } else {
                    write!(writer, " {}{{", span.name())?;
                }
                let ext = span.extensions();
                if let Some(fields) = ext.get::<tracing_subscriber::fmt::FormattedFields<N>>() {
                    if !fields.is_empty() {
                        write!(writer, "{fields}")?;
                    }
                }
                if ansi {
                    write!(writer, "}}\x1b[0m")?;
                } else {
                    write!(writer, "}}")?;
                }
            }
            write!(writer, ":")?;
        }

        // Collect fields with our custom visitor that filters noise.
        let mut visitor = HumanFieldVisitor::new();
        event.record(&mut visitor);

        // Message + visible fields
        write!(writer, " ")?;
        if ansi && (level <= tracing::Level::WARN) {
            write!(
                writer,
                "\x1b[1m{}{}\x1b[0m",
                visitor.message, visitor.fields
            )?;
        } else {
            write!(writer, "{}{}", visitor.message, visitor.fields)?;
        }
        writeln!(writer)
    }
}
