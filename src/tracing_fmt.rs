use tracing_subscriber::registry::LookupSpan;

pub struct TargetFirstFormatter;

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
        use tracing_subscriber::fmt::time::FormatTime;

        let timer = tracing_subscriber::fmt::time::SystemTime;
        timer.format_time(&mut writer)?;

        let level = *event.metadata().level();
        write!(writer, " {level:>5}")?;

        write!(writer, " {}", event.metadata().target())?;

        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                write!(writer, " {}{{", span.name())?;
                let ext = span.extensions();
                if let Some(fields) = ext.get::<tracing_subscriber::fmt::FormattedFields<N>>() {
                    if !fields.is_empty() {
                        write!(writer, "{fields}")?;
                    }
                }
                write!(writer, "}}")?;
            }
            write!(writer, ":")?;
        }

        write!(writer, " ")?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}
