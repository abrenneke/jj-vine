use std::fmt;

use tracing::{Event, field::Visit};
use tracing_subscriber::{
    fmt::{
        FmtContext,
        format::{FormatEvent, FormatFields, Writer},
    },
    registry::LookupSpan,
};

/// Simple formatter that prints the message field without applying level-based
/// coloring. This preserves any ANSI codes already in the message.
#[derive(Default)]
pub struct PlainFormatter {
    display_level: bool,
    display_timestamp: bool,
}

impl PlainFormatter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            display_level: false,
            display_timestamp: false,
        }
    }

    #[must_use]
    pub fn with_level(mut self, display: bool) -> Self {
        self.display_level = display;
        self
    }

    #[must_use]
    pub fn with_timestamp(mut self, display: bool) -> Self {
        self.display_timestamp = display;
        self
    }
}

impl<S, N> FormatEvent<S, N> for PlainFormatter
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut visitor = MessageVisitor { message: None };
        event.record(&mut visitor);
        let message = visitor.message.ok_or(fmt::Error)?;

        // Optionally write timestamp
        if self.display_timestamp {
            let now = jiff::Timestamp::now();
            write!(writer, "{now} ")?;
        }

        // Optionally write level
        if self.display_level {
            let metadata = event.metadata();
            write!(writer, "{:5} ", metadata.level())?;
        }

        // Write the message as-is, preserving any ANSI codes
        writeln!(writer, "{message}")
    }
}

/// A visitor to capture only the `message` field.
struct MessageVisitor {
    message: Option<String>,
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        }
    }
}
