use std::{
    fmt,
    io::{self, Write},
};
use tracing::{
    event::Event,
    field::Field,
    field::Visit,
    span::{Attributes, Id, Record},
    Metadata, Subscriber,
};
use tracing_subscriber::{layer::Context, registry::LookupSpan};

use crate::android::{AndroidWriter, CappedTag};

pub struct Layer {
    tag: CappedTag,
}

impl Layer {
    pub fn new(name: &str) -> io::Result<Self> {
        let tag = CappedTag::new(name.as_bytes())?;
        Ok(Self { tag })
    }
}

impl<S> tracing_subscriber::Layer<S> for Layer
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("unknown span");
        let mut buf = Vec::with_capacity(256);

        let depth = span.parent().into_iter().flat_map(|x| x.scope()).count();

        attrs.record(&mut SpanVisitor::new(&mut buf, depth));

        span.extensions_mut().insert(SpanFields(buf));
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("unknown span");
        let depth = span.parent().into_iter().flat_map(|x| x.scope()).count();
        let mut exts = span.extensions_mut();
        let buf = &mut exts.get_mut::<SpanFields>().expect("missing fields").0;
        values.record(&mut SpanVisitor::new(buf, depth));
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut writer = AndroidWriter::new(event.metadata().level(), &self.tag); //PlatformLogWriter::new(event.metadata().level(), &self.tag);

        event.record(&mut writer);

        // add the target
        let _ = write!(&mut writer, "|> {}", event.metadata().target());

        let maybe_scope = ctx
            .current_span()
            .id()
            .and_then(|id| ctx.span_scope(id).map(|x| x.from_root()));
        if let Some(scope) = maybe_scope {
            write!(&mut writer, ": \n").unwrap();
            for (idx, span) in scope.enumerate() {
                let exts = span.extensions();
                write!(&mut writer, "#{} |> ", idx).unwrap();
                put_metadata(&mut writer, span.metadata());
                if let Some(fields) = exts.get::<SpanFields>() {
                    if fields.0.len() > 0 {
                        write!(&mut writer, " {{\n").unwrap();
                        let _ = writer.write_all(&fields.0[..]);
                        write!(&mut writer, "}}").unwrap();
                    } else {
                        write!(&mut writer, " {{}}").unwrap();
                    }
                } else {
                    write!(&mut writer, " {{}}").unwrap();
                }
                write!(&mut writer, "\n").unwrap();
            }
        }

        // Record event fields
        // TODO: make thius configurable
        // put_metadata(&mut writer, event.metadata(), None);
    }
}
struct SpanFields(Vec<u8>);

struct SpanVisitor<'a> {
    buf: &'a mut Vec<u8>,
    depth: usize,
}

impl<'a> SpanVisitor<'a> {
    fn new(buf: &'a mut Vec<u8>, depth: usize) -> Self {
        Self { buf, depth }
    }
}

impl Visit for SpanVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        write_debug(&mut self.buf, field.name(), value);
        write!(self.buf, "\n").unwrap();
    }
}

impl Visit for AndroidWriter<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            // omit message field value
            let _ = write!(self, "{:?}\n", value);
        } else {
            let _ = write_debug(self, field.name(), value);
            let _ = write!(self, "\n");
        }
    }
}

fn put_metadata(buf: &mut impl Write, meta: &Metadata<'_>) {
    if let Some(file) = meta.file() {
        let _ = write!(buf, "{:?}", file);
    }
    let _ = write!(buf, "@{}", meta.name());
    if let Some(line) = meta.line() {
        let _ = write!(buf, "#{}", line);
    }
}

fn write_debug(buf: &mut impl Write, name: &str, value: &dyn fmt::Debug) {
    let _ = write!(buf, "  {}: {:?},", name, value);
}
