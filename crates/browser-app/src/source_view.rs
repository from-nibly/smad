//! Native whole-file source/comparison reader. GTK owns selection; the gutter is not text.
#[cfg(test)]
#[path = "source_view_tests.rs"]
mod tests;
use super::{highlight, webview::DocumentEvent};
use gtk::{gio, glib, prelude::*};
use smad_core::{
    comparison::{self, ComparisonRow, RowKind},
    snapshots::{ComparisonSpec, SnapshotStore},
    source::{self, SourceText},
};
use smad_protocol::LineRange;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

#[derive(Clone)]
struct Paint {
    start: i32,
    end: i32,
    color: (u8, u8, u8),
}
struct Row {
    coordinate: ComparisonRow,
    start: i32,
    end: i32,
}
struct Rendered {
    text: String,
    rows: Vec<Row>,
    paints: Vec<Paint>,
    changes: Vec<usize>,
    warnings: Vec<String>,
    summary: String,
}
struct Job {
    source: PathBuf,
    comparison: Option<ComparisonSpec>,
    dark: bool,
    revision: u64,
    current: Arc<AtomicU64>,
    reply: mpsc::Sender<(u64, Result<Rendered, String>)>,
}
static WORKER: LazyLock<mpsc::SyncSender<Job>> = LazyLock::new(|| {
    let (send, receive) = mpsc::sync_channel::<Job>(1);
    std::thread::Builder::new()
        .name("smad-source".into())
        .spawn(move || {
            while let Ok(job) = receive.recv() {
                if job.current.load(Ordering::Relaxed) != job.revision {
                    continue;
                }
                let result = prepare(&job);
                if job.current.load(Ordering::Relaxed) == job.revision {
                    let _ = job.reply.send((job.revision, result));
                }
            }
        })
        .expect("source renderer worker");
    send
});

fn contrasting_color(color: (u8, u8, u8), dark: bool) -> (u8, u8, u8) {
    let luminance = |(r, g, b): (u8, u8, u8)| {
        let channel = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    };
    let backgrounds = if dark {
        [(32, 37, 43), (22, 51, 35), (59, 34, 40)]
    } else {
        [(255, 255, 255), (229, 248, 231), (255, 233, 231)]
    };
    for step in 0..=100_u16 {
        let blend = |value: u8| {
            ((u16::from(value) * (100 - step) + if dark { 255 * step } else { 0 }) / 100) as u8
        };
        let adjusted = (blend(color.0), blend(color.1), blend(color.2));
        let foreground = luminance(adjusted);
        if backgrounds.iter().all(|background| {
            let background = luminance(*background);
            (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05) >= 4.7
        }) {
            return adjusted;
        }
    }
    if dark { (255, 255, 255) } else { (0, 0, 0) }
}

fn syntax(source: &SourceText, language: &str, dark: bool) -> Vec<Vec<Paint>> {
    let text = source.display_text();
    let mut lines = vec![Vec::new(); source.lines.len()];
    let mut start = 0;
    let mut colors = std::collections::HashMap::new();
    for (style, text) in highlight::code_spans(&text, language, dark) {
        let end = start + text.chars().count() as i32;
        if let Some(style) = style {
            let original = (style.foreground.r, style.foreground.g, style.foreground.b);
            let color = *colors
                .entry(original)
                .or_insert_with(|| contrasting_color(original, dark));
            let first = source
                .lines
                .partition_point(|line| line.display_chars.end <= start);
            for (index, line) in source.lines.iter().enumerate().skip(first) {
                if line.display_chars.start >= end {
                    break;
                }
                let from = start.max(line.display_chars.start);
                let to = end.min(line.display_chars.end);
                if from < to {
                    lines[index].push(Paint {
                        start: from - line.display_chars.start,
                        end: to - line.display_chars.start,
                        color,
                    });
                }
            }
        }
        start = end;
    }
    lines
}

fn prepare(job: &Job) -> Result<Rendered, String> {
    let store = SnapshotStore::from_environment()?;
    let (old, new, coordinates, changes, summary) = if let Some(spec) = &job.comparison {
        let old = store.read_optional(spec.before.as_ref())?;
        let (new, new_present) = if let Some(path) = &spec.live_after {
            match std::fs::metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    (SourceText::parse(String::new())?, false)
                }
                _ => (source::read_source(std::path::Path::new(path))?, true),
            }
        } else {
            (
                store.read_optional(spec.after.as_ref())?,
                spec.after.is_some(),
            )
        };
        let diff = comparison::compare(&old, &new)?;
        let presence = match (spec.before.is_some(), new_present) {
            (false, true) => "file added",
            (true, false) => "file deleted",
            (false, false) => "both sides absent",
            (true, true) if diff.changes.is_empty() => "no changes",
            (true, true) => "file modified",
        };
        let summary = format!(
            "{} → {} · {presence} · {} changed text blocks · {} old / {} new lines{}",
            spec.before_label,
            spec.after_label,
            diff.changes.len(),
            old.lines.len(),
            new.lines.len(),
            if spec.live_after.is_some() {
                " · live after; fixed base"
            } else {
                " · immutable snapshots"
            }
        );
        (
            old,
            new,
            diff.rows,
            diff.changes.into_iter().map(|range| range.start).collect(),
            summary,
        )
    } else {
        let new = source::read_source(&job.source)?;
        let rows = (1..=new.lines.len())
            .map(|line| ComparisonRow {
                old: None,
                new: Some(line as u32),
                kind: RowKind::Context,
            })
            .collect();
        let summary = format!(
            "{} · {} source lines · read-only",
            job.source.display(),
            new.lines.len()
        );
        (
            SourceText::parse(String::new())?,
            new,
            rows,
            Vec::new(),
            summary,
        )
    };
    let language = job
        .source
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("");
    let old_paints = syntax(&old, language, job.dark);
    let new_paints = syntax(&new, language, job.dark);
    let mut warnings = Vec::new();
    if [&old, &new].iter().any(|source| {
        source.text.len() > 64 * 1024 || source.text.lines().any(|line| line.len() > 4096)
    }) {
        warnings.push("Syntax highlighting skipped for inputs over 64 KiB or lines over 4096 bytes; full source is retained".into());
    }
    let mut summary = summary;
    for (label, source) in [("old", &old), ("new", &new)] {
        if source
            .lines
            .iter()
            .any(|line| line.ending == source::LineEnding::CrLf)
        {
            summary.push_str(&format!(" · {label}: CRLF (displayed as LF)"));
        }
        if source
            .lines
            .last()
            .is_some_and(|line| line.ending == source::LineEnding::None)
        {
            summary.push_str(&format!(" · {label}: no final newline"));
        }
    }
    let mut rendered = Rendered {
        text: String::new(),
        rows: Vec::new(),
        paints: Vec::new(),
        changes,
        warnings,
        summary,
    };
    let mut offset = 0;
    for coordinate in coordinates {
        let (source, paints, index) = if let Some(line) = coordinate.new {
            (&new, &new_paints, line as usize - 1)
        } else {
            (
                &old,
                &old_paints,
                coordinate.old.expect("old row") as usize - 1,
            )
        };
        let content = source.content(index);
        let start = offset;
        rendered.text.push_str(content);
        offset += content.chars().count() as i32;
        if job.comparison.is_some() || source.lines[index].ending != source::LineEnding::None {
            rendered.text.push('\n');
            offset += 1;
        }
        for paint in &paints[index] {
            let end = (start + paint.end).min(offset);
            if start + paint.start < end {
                rendered.paints.push(Paint {
                    start: start + paint.start,
                    end,
                    color: paint.color,
                });
            }
        }
        rendered.rows.push(Row {
            coordinate,
            start,
            end: offset,
        });
    }
    Ok(rendered)
}

pub struct SourceView {
    root: gtk::Box,
    text: gtk::TextView,
    scroll: gtk::ScrolledWindow,
    gutter: gtk::DrawingArea,
    status: gtk::Label,
    source: PathBuf,
    comparison: Option<ComparisonSpec>,
    rendered: RefCell<Option<Rendered>>,
    highlights: RefCell<Vec<LineRange>>,
    old_highlights: RefCell<Vec<LineRange>>,
    current: Arc<AtomicU64>,
    pending: RefCell<Option<Job>>,
    reply: mpsc::Sender<(u64, Result<Rendered, String>)>,
    timer: RefCell<Option<glib::SourceId>>,
    debounce: RefCell<Option<glib::SourceId>>,
    monitor: RefCell<Option<gio::FileMonitor>>,
    first: Cell<bool>,
    last_ready: Cell<u64>,
    navigation: Cell<usize>,
    change_navigation: Cell<usize>,
    dark: bool,
    emit: Rc<dyn Fn(DocumentEvent)>,
}

impl SourceView {
    pub fn new(
        document: &smad_core::Document,
        identity: &str,
        emit: Rc<dyn Fn(DocumentEvent)>,
    ) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.set_widget_name(&format!("source-{identity}"));
        let status = gtk::Label::new(Some("Loading native source…"));
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        status.set_lines(3);
        status.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        status.set_selectable(true);
        root.append(&status);
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let previous = gtk::Button::with_label("Previous highlight");
        let next = gtk::Button::with_label("Next highlight");
        let clear = gtk::Button::with_label("Clear highlights");
        let previous_change = gtk::Button::with_label("Previous change");
        let next_change = gtk::Button::with_label("Next change");
        let reload = gtk::Button::with_label("Reload");
        for button in [&previous, &next, &clear] {
            toolbar.append(button);
        }
        if document.comparison.is_some() {
            toolbar.append(&previous_change);
            toolbar.append(&next_change);
        }
        toolbar.append(&reload);
        root.append(&toolbar);
        let legend = gtk::Label::new(Some(if document.comparison.is_some() {
            "Gutter: OLD | NEW source lines · green +: added · red −: deleted · amber gutter + underline: agent highlight"
        } else {
            "Gutter: original source lines · amber gutter + underline: agent highlight · selection and copied text exclude the gutter"
        }));
        legend.set_xalign(0.0);
        root.append(&legend);
        let text = gtk::TextView::new();
        text.set_editable(false);
        text.set_cursor_visible(true);
        text.set_monospace(true);
        text.set_wrap_mode(gtk::WrapMode::None);
        text.set_left_margin(12);
        text.set_right_margin(24);
        text.set_top_margin(12);
        text.set_bottom_margin(32);
        text.update_property(&[gtk::accessible::Property::Label(&format!(
            "Native source {}",
            document.key.canonical_source_file
        ))]);
        let gutter = gtk::DrawingArea::new();
        gutter.set_content_width(if document.comparison.is_some() {
            150
        } else {
            90
        });
        text.set_gutter(gtk::TextWindowType::Left, Some(&gutter));
        let scroll = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&text)
            .build();
        root.append(&scroll);
        let dark = gtk::Settings::default().is_some_and(|settings| {
            settings.is_gtk_application_prefer_dark_theme()
                || settings
                    .gtk_theme_name()
                    .is_some_and(|name| name.to_lowercase().contains("dark"))
        });
        text.add_css_class("native-source");
        let css = gtk::CssProvider::new();
        css.load_from_data(&format!("textview.native-source {{ font-family: monospace; font-size: 14px; }} textview.native-source text {{ background-color: {}; color: {}; }}", if dark { "#20252b" } else { "#ffffff" }, if dark { "#e6edf3" } else { "#24292f" }));
        #[allow(deprecated)]
        text.style_context()
            .add_provider(&css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        let (reply, receive) = mpsc::channel();
        let view = Rc::new(Self {
            root,
            text,
            scroll,
            gutter,
            status,
            source: PathBuf::from(&document.key.canonical_source_file),
            comparison: document.comparison.clone(),
            rendered: RefCell::new(None),
            highlights: RefCell::new(document.highlights.clone()),
            old_highlights: RefCell::new(document.old_highlights.clone()),
            current: Arc::new(AtomicU64::new(0)),
            pending: RefCell::new(None),
            reply,
            timer: RefCell::new(None),
            debounce: RefCell::new(None),
            monitor: RefCell::new(None),
            first: Cell::new(true),
            last_ready: Cell::new(0),
            navigation: Cell::new(0),
            change_navigation: Cell::new(usize::MAX),
            dark,
            emit,
        });
        for (button, backwards, changes) in [
            (&previous, true, false),
            (&next, false, false),
            (&previous_change, true, true),
            (&next_change, false, true),
        ] {
            let weak = Rc::downgrade(&view);
            button.connect_clicked(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.navigate(backwards, changes);
                }
            });
        }
        let weak = Rc::downgrade(&view);
        clear.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.highlights.borrow_mut().clear();
                view.old_highlights.borrow_mut().clear();
                view.apply_highlights();
                (view.emit)(DocumentEvent::HighlightsCleared);
                if view.last_ready.get() == view.current.load(Ordering::Relaxed) {
                    let warning = {
                        let mut rendered = view.rendered.borrow_mut();
                        rendered.as_mut().map(|rendered| {
                            rendered.warnings.retain(|warning| {
                                !warning.starts_with("new-side highlights exceed")
                                    && !warning.starts_with("old-side highlights exceed")
                            });
                            let warning = rendered.warnings.join("; ");
                            view.status.set_text(&format!(
                                "{}{}",
                                rendered.summary,
                                if warning.is_empty() {
                                    String::new()
                                } else {
                                    format!(" · {warning}")
                                }
                            ));
                            view.status
                                .set_tooltip_text(Some(view.status.text().as_str()));
                            warning
                        })
                    };
                    if let Some(warning) = warning {
                        let title = view
                            .source
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned());
                        (view.emit)(if warning.is_empty() {
                            DocumentEvent::Ready(title)
                        } else {
                            DocumentEvent::ReadyWithWarnings(title, warning)
                        });
                    }
                }
            }
        });
        let weak = Rc::downgrade(&view);
        reload.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.reload();
            }
        });
        let weak = Rc::downgrade(&view);
        view.gutter.set_draw_func(move |_, context, width, _| {
            if let Some(view) = weak.upgrade() {
                view.draw_gutter(context, width);
            }
        });
        let weak = Rc::downgrade(&view);
        view.scroll.vadjustment().connect_value_changed(move |_| {
            if let Some(view) = weak.upgrade() {
                view.gutter.queue_draw();
            }
        });
        let weak = Rc::downgrade(&view);
        *view.timer.borrow_mut() = Some(glib::timeout_add_local(
            Duration::from_millis(40),
            move || {
                let Some(view) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let pending = view.pending.borrow_mut().take();
                if let Some(job) = pending {
                    match WORKER.try_send(job) {
                        Ok(()) => {}
                        Err(mpsc::TrySendError::Full(job)) => {
                            *view.pending.borrow_mut() = Some(job)
                        }
                        Err(mpsc::TrySendError::Disconnected(_)) => (view.emit)(
                            DocumentEvent::Failed("source worker stopped; restart SMAD".into()),
                        ),
                    }
                }
                while let Ok((revision, result)) = receive.try_recv() {
                    if revision == view.current.load(Ordering::Relaxed) {
                        match result {
                            Ok(rendered) => view.present(rendered),
                            Err(error) => {
                                view.status.set_text(&format!(
                                    "{error} · {}",
                                    if view.rendered.borrow().is_some() {
                                        "last good content retained"
                                    } else {
                                        "no content loaded"
                                    }
                                ));
                                (view.emit)(DocumentEvent::Failed(error));
                            }
                        }
                    }
                }
                glib::ControlFlow::Continue
            },
        ));
        let watched = view.comparison.as_ref().map_or_else(
            || Some(view.source.clone()),
            |spec| spec.live_after.as_ref().map(PathBuf::from),
        );
        if let Some(path) = watched
            && let Some(parent) = path.parent()
        {
            match gio::File::for_path(parent)
                .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            {
                Ok(monitor) => {
                    let weak = Rc::downgrade(&view);
                    monitor.connect_changed(move |_, file, other, _| {
                        if file.path().as_ref() != Some(&path)
                            && other.and_then(|file| file.path()).as_ref() != Some(&path)
                        {
                            return;
                        }
                        if let Some(view) = weak.upgrade() {
                            if let Some(timer) = view.debounce.borrow_mut().take() {
                                timer.remove();
                            }
                            let weak = Rc::downgrade(&view);
                            *view.debounce.borrow_mut() = Some(glib::timeout_add_local_once(
                                Duration::from_millis(180),
                                move || {
                                    if let Some(view) = weak.upgrade() {
                                        view.debounce.borrow_mut().take();
                                        view.reload();
                                    }
                                },
                            ));
                        }
                    });
                    *view.monitor.borrow_mut() = Some(monitor);
                }
                Err(error) => view
                    .status
                    .set_text(&format!("Live monitoring unavailable: {error}; use Reload")),
            }
        }
        let weak = Rc::downgrade(&view);
        glib::idle_add_local_once(move || {
            if let Some(view) = weak.upgrade() {
                view.reload();
            }
        });
        view
    }

    pub fn widget(&self) -> gtk::Box {
        self.root.clone()
    }
    pub fn reload(&self) {
        let revision = self.current.fetch_add(1, Ordering::Relaxed) + 1;
        *self.pending.borrow_mut() = Some(Job {
            source: self.source.clone(),
            comparison: self.comparison.clone(),
            dark: self.dark,
            revision,
            current: self.current.clone(),
            reply: self.reply.clone(),
        });
        (self.emit)(DocumentEvent::Loading);
    }
    fn marked(&self, row: &Row) -> bool {
        row.coordinate.new.is_some_and(|line| {
            self.highlights
                .borrow()
                .iter()
                .any(|range| range.contains(line))
        }) || row.coordinate.old.is_some_and(|line| {
            self.old_highlights
                .borrow()
                .iter()
                .any(|range| range.contains(line))
        })
    }
    fn apply_highlights(&self) {
        let buffer = self.text.buffer();
        let tag = buffer
            .tag_table()
            .lookup("agent-highlight")
            .unwrap_or_else(|| {
                buffer
                    .create_tag(
                        Some("agent-highlight"),
                        &[
                            ("underline", &gtk::pango::Underline::Double),
                            (
                                "underline-rgba",
                                &gtk::gdk::RGBA::parse(if self.dark {
                                    "#ffd166"
                                } else {
                                    "#9a6700"
                                })
                                .unwrap(),
                            ),
                        ],
                    )
                    .unwrap()
            });
        buffer.remove_tag(&tag, &buffer.start_iter(), &buffer.end_iter());
        if let Some(rendered) = self.rendered.borrow().as_ref() {
            for row in &rendered.rows {
                if self.marked(row) {
                    buffer.apply_tag(
                        &tag,
                        &buffer.iter_at_offset(row.start),
                        &buffer.iter_at_offset(row.end),
                    );
                }
            }
        }
        self.gutter.queue_draw();
    }
    fn present(self: &Rc<Self>, mut rendered: Rendered) {
        self.last_ready.set(self.current.load(Ordering::Relaxed));
        let position = self.scroll.vadjustment().value();
        let buffer = self.text.buffer();
        buffer.set_text(&rendered.text);
        for paint in &rendered.paints {
            let (r, g, b) = paint.color;
            let name = format!("source-{r}-{g}-{b}");
            let tag = buffer.tag_table().lookup(&name).unwrap_or_else(|| {
                buffer
                    .create_tag(
                        Some(&name),
                        &[("foreground", &format!("#{r:02x}{g:02x}{b:02x}"))],
                    )
                    .unwrap()
            });
            buffer.apply_tag(
                &tag,
                &buffer.iter_at_offset(paint.start),
                &buffer.iter_at_offset(paint.end),
            );
        }
        for (kind, name, color) in [
            (
                RowKind::Added,
                "source-added",
                if self.dark { "#163323" } else { "#e5f8e7" },
            ),
            (
                RowKind::Removed,
                "source-removed",
                if self.dark { "#3b2228" } else { "#ffe9e7" },
            ),
        ] {
            let tag = buffer.tag_table().lookup(name).unwrap_or_else(|| {
                buffer
                    .create_tag(Some(name), &[("paragraph-background", &color)])
                    .unwrap()
            });
            for row in &rendered.rows {
                if row.coordinate.kind == kind {
                    buffer.apply_tag(
                        &tag,
                        &buffer.iter_at_offset(row.start),
                        &buffer.iter_at_offset(row.end),
                    );
                }
            }
        }
        for (side, ranges, limit) in [
            (
                "new",
                &self.highlights,
                rendered
                    .rows
                    .iter()
                    .filter_map(|row| row.coordinate.new)
                    .max()
                    .unwrap_or(0),
            ),
            (
                "old",
                &self.old_highlights,
                rendered
                    .rows
                    .iter()
                    .filter_map(|row| row.coordinate.old)
                    .max()
                    .unwrap_or(0),
            ),
        ] {
            if ranges.borrow().iter().any(|range| range.end > limit) {
                rendered.warnings.push(format!("{side}-side highlights exceed {limit} source lines; only existing source lines are emphasized"));
            }
        }
        if self.monitor.borrow().is_none()
            && self
                .comparison
                .as_ref()
                .is_none_or(|spec| spec.live_after.is_some())
        {
            rendered.warnings.push("Live monitoring unavailable; use Reload (including after recreating a deleted parent directory)".into());
        }
        let warning = rendered.warnings.join("; ");
        self.status.set_text(&format!(
            "{}{}",
            rendered.summary,
            if warning.is_empty() {
                String::new()
            } else {
                format!(" · {warning}")
            }
        ));
        self.status
            .set_tooltip_text(Some(self.status.text().as_str()));
        let first = self.first.replace(false);
        *self.rendered.borrow_mut() = Some(rendered);
        self.apply_highlights();
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(view) = weak.upgrade() {
                if first {
                    view.jump_first();
                } else {
                    view.scroll.vadjustment().set_value(position);
                }
            }
        });
        let title = self
            .source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        (self.emit)(if warning.is_empty() {
            DocumentEvent::Ready(title)
        } else {
            DocumentEvent::ReadyWithWarnings(title, warning)
        });
    }
    fn targets(&self, changes: bool) -> Vec<usize> {
        self.rendered
            .borrow()
            .as_ref()
            .map_or_else(Vec::new, |rendered| {
                if changes {
                    return rendered.changes.clone();
                }
                let mut targets = Vec::new();
                for range in self.highlights.borrow().iter() {
                    if let Some(row) = rendered
                        .rows
                        .iter()
                        .position(|row| row.coordinate.new.is_some_and(|line| range.contains(line)))
                    {
                        targets.push(row);
                    }
                }
                for range in self.old_highlights.borrow().iter() {
                    if let Some(row) = rendered
                        .rows
                        .iter()
                        .position(|row| row.coordinate.old.is_some_and(|line| range.contains(line)))
                    {
                        targets.push(row);
                    }
                }
                targets.sort_unstable();
                targets.dedup();
                targets
            })
    }
    fn jump_first(&self) {
        if let Some(row) = self.targets(false).first() {
            self.jump(*row);
        }
    }
    fn navigate(&self, backwards: bool, changes: bool) {
        let targets = self.targets(changes);
        if targets.is_empty() {
            return;
        }
        let index = if changes {
            &self.change_navigation
        } else {
            &self.navigation
        };
        index.set(if index.get() >= targets.len() {
            if backwards { targets.len() - 1 } else { 0 }
        } else if backwards {
            (index.get() + targets.len() - 1) % targets.len()
        } else {
            (index.get() + 1) % targets.len()
        });
        self.jump(targets[index.get()]);
    }
    fn jump(&self, row: usize) {
        if let Some(rendered) = self.rendered.borrow().as_ref()
            && let Some(row) = rendered.rows.get(row)
        {
            let mut iter = self.text.buffer().iter_at_offset(row.start);
            self.text.scroll_to_iter(&mut iter, 0.15, true, 0.0, 0.15);
        }
    }
    fn draw_gutter(&self, context: &gtk::cairo::Context, width: i32) {
        let rendered = self.rendered.borrow();
        let Some(rendered) = rendered.as_ref() else {
            return;
        };
        let visible = self.text.visible_rect();
        let buffer = self.text.buffer();
        context.select_font_face(
            "monospace",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Normal,
        );
        context.set_font_size(12.0);
        let visible_offset = self
            .text
            .iter_at_location(0, visible.y())
            .map_or(0, |iter| iter.offset());
        let first = rendered
            .rows
            .partition_point(|row| row.end <= visible_offset);
        for row in rendered.rows.iter().skip(first) {
            let (y, height) = self.text.line_yrange(&buffer.iter_at_offset(row.start));
            if y + height < visible.y() {
                continue;
            }
            if y > visible.y() + visible.height() {
                break;
            }
            let (_, screen_y) = self
                .text
                .buffer_to_window_coords(gtk::TextWindowType::Left, 0, y);
            let color = match row.coordinate.kind {
                RowKind::Added => Some((0.1, 0.55, 0.25)),
                RowKind::Removed => Some((0.8, 0.2, 0.2)),
                RowKind::Context => None,
            };
            if let Some((r, g, b)) = color {
                context.set_source_rgba(r, g, b, 0.22);
                context.rectangle(0.0, screen_y as f64, width as f64, height as f64);
                let _ = context.fill();
            }
            if self.marked(row) {
                context.set_source_rgb(0.9, 0.62, 0.05);
                context.rectangle(0.0, screen_y as f64, 5.0, height as f64);
                let _ = context.fill();
            }
            if self.dark {
                context.set_source_rgb(0.8, 0.8, 0.8);
            } else {
                context.set_source_rgb(0.3, 0.3, 0.3);
            }
            let number =
                |number: Option<u32>| number.map(|number| number.to_string()).unwrap_or_default();
            let sign = match row.coordinate.kind {
                RowKind::Context => " ",
                RowKind::Added => "+",
                RowKind::Removed => "−",
            };
            let label = if self.comparison.is_some() {
                format!(
                    "{:>6} {:>6} {sign}",
                    number(row.coordinate.old),
                    number(row.coordinate.new)
                )
            } else {
                format!("{:>7}", number(row.coordinate.new))
            };
            context.move_to(8.0, (screen_y + height - 3) as f64);
            let _ = context.show_text(&label);
        }
    }
}
impl Drop for SourceView {
    fn drop(&mut self) {
        self.current.fetch_add(1, Ordering::Relaxed);
        if let Some(timer) = self.timer.get_mut().take() {
            timer.remove();
        }
        if let Some(timer) = self.debounce.get_mut().take() {
            timer.remove();
        }
        if let Some(monitor) = self.monitor.get_mut().take() {
            monitor.cancel();
        }
    }
}
