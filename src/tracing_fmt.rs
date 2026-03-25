use std::fmt::Write as _;
use std::fs::File;
use std::fs::OpenOptions;
use std::fs::{self};
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use chrono::Local;
use parking_lot::Mutex;
use tracing_subscriber::registry::LookupSpan;

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
            if let Ok(file) = open_log_file(&self.dir, &self.prefix, &now_hour) {
                state.file = file;
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

#[derive(Clone, Default)]
pub struct TargetFirstFormatter;

impl TargetFirstFormatter {
    pub fn new() -> Self {
        Self
    }
}

const KEEP_ZERO_KEYS: &[&str] = &[
    "elapsed_ms",
    "discover_ms",
    "delay_ms",
    "ttft_ms",
    "bytes",
    "input_bytes",
    "output_bytes",
    "response_bytes",
];
const META_KEYS: &[&str] = &[
    "tool_call_id",
    "account_id",
    "sender_id",
    "parent_run_id",
    "operation",
    "tool_kind",
    "message_id",
    "resource_id",
    "node_id",
];
const INDENT: &str = "               ";
const WIDTH: usize = 110;

#[derive(Clone, Debug)]
struct Field {
    name: String,
    plain: String,
    rendered: String,
}

impl Field {
    fn new(name: &str, plain: String, rendered: String) -> Self {
        Self {
            name: name.to_string(),
            plain,
            rendered,
        }
    }
}

struct EventData {
    message: String,
    stage: String,
    status: String,
    run_id: String,
    session_id: String,
    turn: Option<u32>,
    fields: Vec<Field>,
}

impl EventData {
    fn new() -> Self {
        Self {
            message: String::new(),
            stage: String::new(),
            status: String::new(),
            run_id: String::new(),
            session_id: String::new(),
            turn: None,
            fields: Vec::new(),
        }
    }

    fn default_message(&self) -> String {
        format!("{} {}", self.stage, self.status)
    }
}

impl tracing::field::Visit for EventData {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            write!(self.message, "{value:?}").ok();
            return;
        }
        let rendered = format!("{value:?}");
        self.fields
            .push(Field::new(field.name(), rendered.clone(), rendered));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => self.message.push_str(value),
            "stage" => self.stage = value.to_string(),
            "status" => self.status = value.to_string(),
            "run_id" => self.run_id = value.to_string(),
            "session_id" => self.session_id = value.to_string(),
            _ => {
                let rendered = if needs_quotes(value) {
                    format!("\"{value}\"")
                } else {
                    value.to_string()
                };
                self.fields
                    .push(Field::new(field.name(), value.to_string(), rendered));
            }
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == "turn" {
            self.turn = Some(value as u32);
            return;
        }
        if value == 0 && !KEEP_ZERO_KEYS.contains(&field.name()) {
            return;
        }
        let text = value.to_string();
        self.fields
            .push(Field::new(field.name(), text.clone(), text));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        if value == 0 && !KEEP_ZERO_KEYS.contains(&field.name()) {
            return;
        }
        let text = value.to_string();
        self.fields
            .push(Field::new(field.name(), text.clone(), text));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        if value == 0.0 && !KEEP_ZERO_KEYS.contains(&field.name()) {
            return;
        }
        let text = value.to_string();
        self.fields
            .push(Field::new(field.name(), text.clone(), text));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        let text = value.to_string();
        self.fields
            .push(Field::new(field.name(), text.clone(), text));
    }
}

#[derive(Clone)]
struct Fields {
    items: Vec<Field>,
}

impl Fields {
    fn new(items: Vec<Field>) -> Self {
        Self { items }
    }

    fn take(&mut self, name: &str) -> Option<Field> {
        let idx = self.items.iter().position(|field| field.name == name)?;
        Some(self.items.remove(idx))
    }

    fn take_many(&mut self, names: &[&str]) -> Vec<Field> {
        let mut out = Vec::new();
        for name in names {
            if let Some(field) = self.take(name) {
                out.push(field);
            }
        }
        out
    }

    fn take_meta(&mut self) -> Vec<Field> {
        let mut out = self.take_many(META_KEYS);
        let mut idx = 0;
        while idx < self.items.len() {
            let name = self.items[idx].name.as_str();
            if name.ends_with("_id") || name == "request_id" || name == "trace_id" {
                out.push(self.items.remove(idx));
            } else {
                idx += 1;
            }
        }
        out
    }

    fn drain(self) -> Vec<Field> {
        self.items
    }
}

struct Rendered {
    header: String,
    lines: Vec<String>,
}

fn needs_quotes(value: &str) -> bool {
    value.is_empty()
        || value.contains(' ')
        || value.contains('\t')
        || value.contains('"')
        || value.contains('=')
}

fn format_ms(value: &str) -> String {
    let Ok(ms) = value.parse::<u64>() else {
        return value.to_string();
    };
    if ms < 1000 {
        return format!("{ms}ms");
    }
    format!("{}.{:03}s", ms / 1000, ms % 1000)
}

fn format_bytes(value: &str) -> String {
    let Ok(bytes) = value.parse::<u64>() else {
        return value.to_string();
    };
    if bytes < 1024 {
        return format!("{bytes}B");
    }
    if bytes < 1024 * 1024 {
        return format!("{:.1}KB", bytes as f64 / 1024.0);
    }
    format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
}

fn display(field: &Field) -> String {
    match field.name.as_str() {
        "elapsed_ms" | "discover_ms" | "delay_ms" | "ttft_ms" => format_ms(&field.plain),
        "bytes" | "input_bytes" | "output_bytes" | "response_bytes" => format_bytes(&field.plain),
        _ => field.rendered.clone(),
    }
}

fn short_run_id(run_id: &str) -> String {
    run_id.chars().take(8).collect()
}

fn session_label(session_id: &str) -> Option<String> {
    let (base, _) = session_id.split_once('#').unwrap_or((session_id, ""));
    let parts: Vec<_> = base.split(':').collect();
    if parts.len() >= 3 {
        return Some(format!("{} / chat {}", parts[0], parts[2]));
    }
    None
}

fn iteration(message: &str, fallback: Option<u32>) -> u32 {
    if let Some(idx) = message.find("iter-") {
        let digits: String = message[idx + 5..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        if let Ok(value) = digits.parse() {
            return value;
        }
    }
    fallback.unwrap_or(0)
}

fn humanize(value: &str) -> String {
    value.replace('_', " ")
}

fn default_header(stage: &str, status: &str) -> String {
    match status {
        "started" => format!("{} start", humanize(stage)),
        "completed" => format!("{} done", humanize(stage)),
        "failed" => format!("{} failed", humanize(stage)),
        "config" => format!("{} config", humanize(stage)),
        _ => format!("{} {}", humanize(stage), humanize(status)),
    }
}

fn run_ctx(data: &EventData) -> String {
    if data.run_id.is_empty() {
        return String::new();
    }
    let mut ctx = format!("[{}]", short_run_id(&data.run_id));
    if let Some(turn) = data.turn {
        write!(ctx, " [t{turn}]").ok();
    }
    ctx
}

fn wrap(prefix: &str, parts: &[String]) -> Vec<String> {
    if parts.is_empty() {
        return Vec::new();
    }
    let continuation = " ".repeat(prefix.chars().count());
    let prefix_len = prefix.chars().count();
    let mut current = prefix.to_string();
    let mut lines = Vec::new();
    for part in parts {
        let sep = usize::from(current.chars().count() > prefix_len);
        if current.chars().count() + sep + part.chars().count() > WIDTH
            && current.chars().count() > prefix_len
        {
            lines.push(current);
            current = continuation.clone();
        }
        if current.chars().count() > prefix_len {
            current.push(' ');
        }
        current.push_str(part);
    }
    lines.push(current);
    lines
}

fn pair(field: Field) -> String {
    format!("{}={}", label(&field.name), display(&field))
}

fn pairs(fields: Vec<Field>) -> Vec<String> {
    fields.into_iter().map(pair).collect()
}

fn label(name: &str) -> &str {
    match name {
        "history_messages" => "history",
        "tool_count" => "tools",
        "message_count" => "messages",
        "max_context_tokens" => "max_ctx",
        "prompt_tokens" => "prompt",
        "completion_tokens" => "comp",
        "chunk_count" => "chunks",
        "tool_calls" => "calls",
        "merged_count" => "merged",
        "elapsed_ms" => "elapsed",
        "discover_ms" => "elapsed",
        "delay_ms" => "delay",
        "ttft_ms" => "ttft",
        _ => name,
    }
}

fn push_text(lines: &mut Vec<String>, prefix: &str, label: &str, text: &str) {
    let line = if label.is_empty() {
        format!("\"{text}\"")
    } else {
        format!("{label}\"{text}\"")
    };
    lines.push(format!("{prefix}{line}"));
}

fn push_meta(lines: &mut Vec<String>, prefix: &str, fields: Vec<Field>) {
    let parts = pairs(fields);
    if !parts.is_empty() {
        lines.extend(wrap(&format!("{prefix}meta: "), &parts));
    }
}

fn render_channel(data: &EventData, mut fields: Fields) -> Rendered {
    let channel = fields
        .take("channel_type")
        .map(|field| field.plain)
        .unwrap_or_else(|| "channel".to_string());
    let header = match data.status.as_str() {
        "accepted" => format!("{channel} message"),
        "sent" => format!("{channel} reply"),
        _ => format!("{channel} {}", humanize(&data.status)),
    };
    let mut lines = wrap("", &pairs(fields.take_many(&["chat_id", "user_id"])));
    if let Some(text) = fields
        .take("input_preview")
        .or_else(|| fields.take("output_preview"))
        .or_else(|| fields.take("input"))
        .or_else(|| fields.take("output"))
    {
        push_text(&mut lines, "", "", &text.plain);
    }
    let mut meta = fields.take_meta();
    meta.extend(fields.take_many(&["bytes", "merged_count", "input_bytes", "output_bytes"]));
    meta.extend(fields.drain());
    push_meta(&mut lines, "", meta);
    Rendered { header, lines }
}

fn render_run_start(data: &EventData, mut fields: Fields) -> Rendered {
    let mut header = format!("run start {}", run_ctx(data));
    if header.ends_with(' ') {
        header.pop();
    }
    let mut lines = Vec::new();
    if let Some(session) = session_label(&data.session_id) {
        lines.push(session);
    }
    lines.extend(wrap(
        "",
        &pairs(fields.take_many(&["user_id", "run_index", "bytes"])),
    ));
    if let Some(input) = fields
        .take("input_preview")
        .or_else(|| fields.take("input"))
    {
        push_text(&mut lines, "", "input=", &input.plain);
    }
    let mut meta = Vec::new();
    if let Some(parent) = fields.take("parent_run_id") {
        meta.push(Field::new(
            "parent_run_id",
            if parent.plain.is_empty() {
                "-".to_string()
            } else {
                parent.plain
            },
            String::new(),
        ));
    }
    meta.extend(fields.take_meta());
    push_meta(&mut lines, "", meta);
    lines.extend(wrap(
        "",
        &pairs(fields.take_many(&["history_messages", "tool_count"])),
    ));
    Rendered { header, lines }
}

fn render_run_end(data: &EventData, mut fields: Fields) -> Rendered {
    let header = format!("run end {}", run_ctx(data));
    let mut parts = vec![format!(
        "status={}",
        if data.status == "failed" {
            "error"
        } else {
            data.status.as_str()
        }
    )];
    parts.extend(pairs(fields.take_many(&[
        "elapsed_ms",
        "tokens",
        "prompt_tokens",
        "completion_tokens",
        "iterations",
        "event_count",
        "stop_reason",
        "ttft_ms",
    ])));
    let mut lines = wrap("", &parts);
    if let Some(error) = fields.take("error") {
        lines.push(format!("error={}", display(&error)));
    }
    push_meta(&mut lines, "", fields.take_meta());
    Rendered { header, lines }
}

fn render_prompt(data: &EventData, mut fields: Fields) -> Rendered {
    let size = fields
        .take("bytes")
        .map(|field| display(&field))
        .unwrap_or_else(|| "-".to_string());
    let header = format!("prompt build {} size={size}", run_ctx(data));
    let lines = wrap(
        "",
        &pairs(fields.take_many(&["history_messages", "tool_count", "user_id"])),
    );
    Rendered { header, lines }
}

fn render_turn(data: &EventData, mut fields: Fields) -> Rendered {
    let iter = iteration(&data.message, data.turn);
    let strategy = fields
        .take("tool_strategy")
        .map(|field| field.plain)
        .unwrap_or_else(|| "default".to_string());
    let header = format!(
        "turn start {} iter={iter} strategy={strategy}",
        run_ctx(data)
    );
    let lines = wrap(
        "",
        &pairs(fields.take_many(&["message_count", "max_context_tokens"])),
    );
    Rendered { header, lines }
}

fn render_llm(data: &EventData, mut fields: Fields) -> Rendered {
    let title = if data.status == "request" {
        "llm request".to_string()
    } else {
        let finish = fields
            .take("finish_reason")
            .map(|field| field.plain)
            .unwrap_or_else(|| data.status.clone());
        format!("llm {finish}")
    };
    let header = format!("{title} {}", run_ctx(data)).trim().to_string();
    let detail_fields = if data.status == "request" {
        fields.take_many(&[
            "model",
            "tool_strategy",
            "rows",
            "bytes",
            "tool_count",
            "attempt",
        ])
    } else {
        fields.take_many(&[
            "elapsed_ms",
            "tokens",
            "ttft_ms",
            "model",
            "provider",
            "bytes",
            "chunk_count",
            "attempt",
            "tool_calls",
        ])
    };
    let mut lines = wrap("", &pairs(detail_fields));
    if let Some(error) = fields.take("error") {
        lines.push(format!("error={}", display(&error)));
    }
    push_meta(&mut lines, "", fields.take_meta());
    Rendered { header, lines }
}

fn render_tool(data: &EventData, mut fields: Fields) -> Rendered {
    let name = fields
        .take("tool_name")
        .map(|field| field.plain)
        .unwrap_or_else(|| "tool".to_string());
    let status = if data.status == "started" {
        "start"
    } else if data.status == "failed" {
        "failed"
    } else {
        "done"
    };
    let header = format!("tool {name} {status} {}", run_ctx(data))
        .trim()
        .to_string();
    let mut lines = wrap(
        "",
        &pairs(fields.take_many(&["elapsed_ms", "summary", "bytes"])),
    );
    if let Some(error) = fields.take("error") {
        lines.push(format!("error={}", display(&error)));
    }
    push_meta(
        &mut lines,
        "",
        fields.take_many(&["tool_call_id", "tool_kind"]),
    );
    push_meta(&mut lines, "", fields.take_meta());
    Rendered { header, lines }
}

fn render_event_log(data: &EventData, mut fields: Fields) -> Rendered {
    let header = if data.message.trim().is_empty() {
        "event".to_string()
    } else {
        data.message.trim().to_string()
    };
    let mut lines = wrap(
        "",
        &pairs(fields.take_many(&["pattern", "files", "count", "path"])),
    );
    push_meta(&mut lines, "", fields.take_meta());
    Rendered { header, lines }
}

fn render_generic(data: &EventData, mut fields: Fields) -> Rendered {
    let title = if data.message.trim().is_empty() || data.message.trim() == data.default_message() {
        default_header(&data.stage, &data.status)
    } else {
        data.message.trim().to_string()
    };
    let header = if data.run_id.is_empty() {
        title
    } else {
        format!("{title} {}", run_ctx(data))
    };
    let meta = fields.take_meta();
    let mut lines = wrap("", &pairs(fields.drain()));
    push_meta(&mut lines, "", meta);
    Rendered { header, lines }
}

fn render(data: &EventData) -> Option<Rendered> {
    let fields = Fields::new(data.fields.clone());
    let rendered = match (data.stage.as_str(), data.status.as_str()) {
        ("channel", _) => render_channel(data, fields),
        ("run", "started") => render_run_start(data, fields),
        ("run", "completed" | "failed") => render_run_end(data, fields),
        ("prompt", "built") => render_prompt(data, fields),
        ("turn", "started") => render_turn(data, fields),
        ("llm", "request" | "completed" | "failed") => render_llm(data, fields),
        ("tool", "started" | "completed" | "failed") => render_tool(data, fields),
        ("event", "log") => render_event_log(data, fields),
        _ => render_generic(data, fields),
    };
    Some(rendered)
}

impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for TargetFirstFormatter
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let level = *event.metadata().level();
        let mut data = EventData::new();
        event.record(&mut data);
        let Some(rendered) = render(&data) else {
            return Ok(());
        };

        write!(writer, "{}", Local::now().format("%H:%M:%S%.3f"))?;
        write!(writer, " {:>5}", level)?;
        write!(writer, "  {}", rendered.header)?;
        if rendered.lines.is_empty() {
            writeln!(writer)
        } else {
            writeln!(writer)?;
            for line in rendered.lines {
                writeln!(writer, "{INDENT}{line}")?;
            }
            Ok(())
        }
    }
}
