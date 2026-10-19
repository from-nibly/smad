//! Bounded native GTK reader. No HTML conversion, embedded browser, or executable Markdown.
use super::{
    highlight,
    markdown_mapping::{Lines, SourceMap},
    mermaid, table,
    webview::DocumentEvent,
};
use gtk::{gdk, gio, glib, prelude::*};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use smad_core::markdown::{self, LinkTarget};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

type Navigate = Rc<dyn Fn(String, Option<String>) -> Result<(), String>>;

pub struct MarkdownView {
    weak: std::rc::Weak<Self>,
    revision: Cell<u64>,
    job: RefCell<Option<(merman::OperationControl, glib::SourceId)>>,
    attachments: RefCell<Vec<Embedded>>,
    source_map: RefCell<SourceMap>,
    poc_ranges: RefCell<Vec<smad_protocol::LineRange>>,
    range_entry: gtk::Entry,
    range_cursor: Cell<usize>,
    range_jump_pending: Cell<bool>,
    mapping_warning: RefCell<Option<String>>,
    read_error: RefCell<Option<String>>,
    poc_status: gtk::Label,
    poc_source: gtk::TextView,
    root: gtk::Box,
    text: gtk::TextView,
    status: gtk::Label,
    link_menu: gtk::MenuButton,
    scroll: gtk::ScrolledWindow,
    source: PathBuf,
    content: RefCell<Option<String>>,
    warnings: RefCell<Option<String>>,
    anchors: RefCell<HashMap<String, gtk::TextMark>>,
    links: RefCell<Vec<(i32, i32, String)>>,
    pending_heading: RefCell<Option<String>>,
    monitor: RefCell<Option<gio::FileMonitor>>,
    debounce: RefCell<Option<glib::SourceId>>,
    emit: Rc<dyn Fn(DocumentEvent)>,
    navigate: Navigate,
}

impl MarkdownView {
    pub fn new(
        source: &Path,
        identity: &str,
        emit: Rc<dyn Fn(DocumentEvent)>,
        navigate: Navigate,
    ) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_widget_name(&format!("markdown-{identity}"));
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        toolbar.set_margin_start(16);
        toolbar.set_margin_end(16);
        toolbar.set_margin_top(8);
        toolbar.set_margin_bottom(8);
        let status = gtk::Label::new(Some("Loading native Markdown…"));
        status.set_xalign(0.0);
        status.set_hexpand(true);
        status.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        status.set_tooltip_text(source.to_str());
        let reload = gtk::Button::from_icon_name("view-refresh-symbolic");
        reload.set_tooltip_text(Some("Reload Markdown from disk"));
        toolbar.append(&status);
        let link_menu = gtk::MenuButton::builder()
            .label("Links")
            .tooltip_text("Document links (keyboard accessible)")
            .build();
        toolbar.append(&link_menu);
        toolbar.append(&reload);
        root.append(&toolbar);
        let text = gtk::TextView::new();
        text.set_editable(false);
        text.set_cursor_visible(true);
        text.set_wrap_mode(gtk::WrapMode::WordChar);
        text.set_left_margin(36);
        text.set_right_margin(36);
        text.set_top_margin(24);
        text.set_bottom_margin(48);
        text.set_pixels_above_lines(3);
        text.set_pixels_below_lines(3);
        text.add_css_class("native-markdown");
        text.update_property(&[gtk::accessible::Property::Label(&format!(
            "Native Markdown: {}",
            source.display()
        ))]);
        let css = gtk::CssProvider::new();
        css.load_from_data("textview.native-markdown { font-family: sans-serif; font-size: 17px; } textview.native-markdown text { background-color: @theme_base_color; color: @theme_text_color; }");
        #[allow(deprecated)]
        text.style_context()
            .add_provider(&css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        let scroll = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&text)
            .build();
        root.append(&scroll);
        let view = Rc::new_cyclic(|weak| Self {
            weak: weak.clone(),
            revision: Cell::new(0),
            job: RefCell::new(None),
            attachments: RefCell::new(Vec::new()),
            source_map: RefCell::new(SourceMap::default()),
            poc_ranges: RefCell::new(
                std::env::var("SMAD_MARKDOWN_MAPPING_LINES")
                    .ok()
                    .and_then(|s| smad_protocol::parse_ranges(&s).ok())
                    .unwrap_or_default(),
            ),
            range_entry: gtk::Entry::new(),
            range_cursor: Cell::new(0),
            range_jump_pending: Cell::new(false),
            mapping_warning: RefCell::new(None),
            read_error: RefCell::new(None),
            poc_status: gtk::Label::new(None),
            poc_source: gtk::TextView::new(),
            root,
            text,
            status,
            link_menu,
            scroll,
            source: source.to_owned(),
            content: RefCell::new(None),
            warnings: RefCell::new(None),
            anchors: RefCell::new(HashMap::new()),
            links: RefCell::new(Vec::new()),
            pending_heading: RefCell::new(None),
            monitor: RefCell::new(None),
            debounce: RefCell::new(None),
            emit,
            navigate,
        });
        {
            let controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            let entry = view.range_entry.clone();
            entry.set_placeholder_text(Some("12-24,48 · Enter applies; empty clears"));
            entry.set_hexpand(true);
            controls.append(&gtk::Label::new(Some("Source highlights")));
            entry.set_text(&smad_protocol::format_ranges(&view.poc_ranges.borrow()));
            controls.append(&entry);
            for (label, backwards) in [("Previous", true), ("Next", false)] {
                let button = gtk::Button::with_label(label);
                let weak = Rc::downgrade(&view);
                button.connect_clicked(move |_| {
                    if let Some(view) = weak.upgrade() {
                        let count = view.poc_ranges.borrow().len();
                        if count > 0 {
                            view.range_cursor.set(
                                (view.range_cursor.get() + if backwards { count - 1 } else { 1 })
                                    % count,
                            );
                            view.scroll_poc_highlight();
                        }
                    }
                });
                controls.append(&button);
            }
            let clear = gtk::Button::with_label("Clear");
            let input = entry.clone();
            clear.connect_clicked(move |_| {
                input.set_text("");
                input.emit_activate();
            });
            controls.append(&clear);
            view.root.insert_child_after(&controls, Some(&toolbar));
            view.poc_status.set_wrap(true);
            view.root
                .insert_child_after(&view.poc_status, Some(&controls));
            if std::env::var_os("SMAD_MARKDOWN_MAPPING_POC").is_some() {
                view.poc_source.set_editable(false);
                view.poc_source.set_monospace(true);
                let source_scroll = gtk::ScrolledWindow::builder()
                    .height_request(220)
                    .child(&view.poc_source)
                    .build();
                let source_expander = gtk::Expander::builder()
                    .label("POC inspector: original source with line numbers")
                    .child(&source_scroll)
                    .build();
                view.root
                    .insert_child_after(&source_expander, Some(&view.poc_status));
            }
            let weak = Rc::downgrade(&view);
            entry.connect_activate(move |entry| {
                if let Some(view) = weak.upgrade() {
                    let ranges = if entry.text().is_empty() {
                        Ok(Vec::new())
                    } else {
                        smad_protocol::parse_ranges(&entry.text())
                    };
                    match ranges {
                        Ok(ranges) => {
                            view.set_highlights(ranges.clone());
                            (view.emit)(DocumentEvent::HighlightsChanged(ranges));
                            view.ready_status();
                            view.emit_ready();
                        }
                        Err(error) => view.poc_status.set_text(&error),
                    }
                }
            });
        }
        let weak = Rc::downgrade(&view);
        reload.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.reload();
            }
        });
        let weak = Rc::downgrade(&view);
        let click = gtk::GestureClick::new();
        click.set_button(1);
        click.connect_released(move |_, presses, x, y| {
            if presses != 1 {
                return;
            }
            if let Some(view) = weak.upgrade() {
                if view.text.buffer().selection_bounds().is_some() {
                    return;
                }
                let (x, y) = view.text.window_to_buffer_coords(
                    gtk::TextWindowType::Widget,
                    x as i32,
                    y as i32,
                );
                if let Some(iter) = view.text.iter_at_location(x, y) {
                    let href = view
                        .links
                        .borrow()
                        .iter()
                        .find(|(start, end, _)| *start <= iter.offset() && iter.offset() < *end)
                        .map(|(_, _, link)| link.clone());
                    if let Some(href) = href {
                        view.follow_link(&href);
                    }
                }
            }
        });
        view.text.add_controller(click);
        let weak = Rc::downgrade(&view);
        view.text.connect_copy_clipboard(move |text| {
            if let Some(view) = weak.upgrade()
                && let Some((start, end)) = text.buffer().selection_bounds()
            {
                text.clipboard()
                    .set_text(&view.selection_text(&start, &end));
                text.stop_signal_emission_by_name("copy-clipboard");
            }
        });
        let weak = Rc::downgrade(&view);
        view.link_menu.set_create_popup_func(move |menu| {
            let Some(view) = weak.upgrade() else {
                return;
            };
            let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
            let popover = gtk::Popover::new();
            let mut seen = std::collections::HashSet::new();
            for (_, _, href) in view.links.borrow().iter() {
                if !seen.insert(href.clone()) {
                    continue;
                }
                let button = gtk::Button::with_label(href);
                let href = href.clone();
                let weak = Rc::downgrade(&view);
                let popup = popover.downgrade();
                button.connect_clicked(move |_| {
                    if let Some(popup) = popup.upgrade() {
                        popup.popdown();
                    }
                    if let Some(view) = weak.upgrade() {
                        view.follow_link(&href);
                    }
                });
                list.append(&button);
            }
            if seen.is_empty() {
                list.append(&gtk::Label::new(Some("No links in this document")));
            }
            let scroll = gtk::ScrolledWindow::builder()
                .min_content_width(280)
                .max_content_height(360)
                .propagate_natural_height(true)
                .child(&list)
                .build();
            popover.set_child(Some(&scroll));
            menu.set_popover(Some(&popover));
        });
        // Watching the parent, not the file inode, survives editors' atomic rename saves.
        if let Some(parent) = source.parent() {
            match gio::File::for_path(parent)
                .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            {
                Ok(monitor) => {
                    let weak = Rc::downgrade(&view);
                    monitor.connect_changed(move |_, file, other, _| {
                        if let Some(view) = weak.upgrade()
                            && (file.path().as_deref() == Some(&view.source)
                                || other.and_then(|f| f.path()).as_deref() == Some(&view.source))
                        {
                            if let Some(timer) = view.debounce.borrow_mut().take() {
                                timer.remove();
                            }
                            let weak = Rc::downgrade(&view);
                            *view.debounce.borrow_mut() = Some(glib::timeout_add_local_once(
                                Duration::from_millis(180),
                                move || {
                                    if let Some(view) = weak.upgrade() {
                                        view.debounce.borrow_mut().take();
                                        view.refresh();
                                    }
                                },
                            ));
                        }
                    });
                    *view.monitor.borrow_mut() = Some(monitor);
                }
                Err(error) => eprintln!("Markdown file watch unavailable: {error}"),
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
        self.content.borrow_mut().take();
        self.refresh();
    }

    fn refresh(&self) {
        self.cancel_job();
        self.revision.set(self.revision.get().wrapping_add(1));
        let content = match markdown::read_markdown(&self.source) {
            Ok(content) => content,
            Err(error) => {
                self.failure(&error);
                return;
            }
        };
        if content.lines().count() > 20_000 {
            self.failure("Markdown limit: 20,000 source lines");
            return;
        }
        self.read_error.borrow_mut().take();
        if self.content.borrow().as_ref() == Some(&content) {
            self.ready_status();
            self.emit_ready();
            return;
        }
        if mermaid::sources(&content).is_empty() {
            self.apply_content(content, mermaid::Diagrams::new());
            return;
        }
        let (control, receiver) = match mermaid::prepare(&content) {
            Ok(job) => job,
            Err(error) => {
                self.failure(&error);
                return;
            }
        };
        self.status
            .set_text("Rendering native diagrams… previous content retained");
        let weak = self.weak.clone();
        let revision = self.revision.get();
        let mut content = Some(content);
        let poll = glib::timeout_add_local(Duration::from_millis(16), move || {
            let Some(view) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if view.revision.get() != revision {
                return glib::ControlFlow::Break;
            }
            match receiver.try_recv() {
                Ok(result) => {
                    view.job.borrow_mut().take();
                    match result {
                        Ok(diagrams) => {
                            view.apply_content(content.take().expect("one render result"), diagrams)
                        }
                        Err(error) => view.failure(&error),
                    }
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => {
                    view.job.borrow_mut().take();
                    view.failure("diagram worker disconnected");
                    glib::ControlFlow::Break
                }
            }
        });
        *self.job.borrow_mut() = Some((control, poll));
    }

    fn cancel_job(&self) {
        if let Some((control, poll)) = self.job.borrow_mut().take() {
            control.cancel();
            poll.remove();
        }
    }

    fn selection_text(&self, start: &gtk::TextIter, end: &gtk::TextIter) -> String {
        let buffer = self.text.buffer();
        let replacements: HashMap<_, _> = self
            .attachments
            .borrow()
            .iter()
            .map(|item| {
                (
                    buffer.iter_at_child_anchor(&item.anchor).offset(),
                    item.copy.clone(),
                )
            })
            .collect();
        let mut output = String::new();
        let mut iter = *start;
        while iter.offset() < end.offset() {
            if let Some(text) = replacements.get(&iter.offset()) {
                output.push_str(text);
            } else {
                output.push(iter.char());
            }
            if !iter.forward_char() {
                break;
            }
        }
        output
    }

    fn apply_content(&self, content: String, diagrams: mermaid::Diagrams) {
        let weak = self.weak.clone();
        let follow: Rc<dyn Fn(&str)> = Rc::new(move |href| {
            if let Some(view) = weak.upgrade() {
                view.follow_link(href);
            }
        });
        let rendered = match render(&content, &self.source, &diagrams, follow) {
            Ok(rendered) => rendered,
            Err(error) => {
                self.failure(&error);
                return;
            }
        };
        let adjustment = self.scroll.vadjustment();
        let previous = adjustment.value();
        for item in self.attachments.borrow_mut().drain(..) {
            self.text.remove(&item.widget);
        }
        self.text.set_buffer(Some(&rendered.buffer));
        for item in &rendered.attachments {
            self.text.add_child_at_anchor(&item.widget, &item.anchor);
        }
        *self.attachments.borrow_mut() = rendered.attachments;
        *self.source_map.borrow_mut() = rendered.source_map;
        self.apply_poc_highlights();
        *self.anchors.borrow_mut() = rendered.anchors;
        *self.links.borrow_mut() = rendered.links;
        self.link_menu.set_popover(None::<&gtk::Popover>);
        if std::env::var_os("SMAD_MARKDOWN_MAPPING_POC").is_some() {
            let numbered: String = content
                .lines()
                .enumerate()
                .map(|(i, line)| format!("{:4}  {line}\n", i + 1))
                .collect();
            self.poc_source.buffer().set_text(&numbered);
        }
        *self.content.borrow_mut() = Some(content);
        *self.warnings.borrow_mut() = (rendered.diagram_errors > 0).then(|| {
            format!(
                "{} diagram error(s); see source and diagnostics",
                rendered.diagram_errors
            )
        });
        self.ready_status();
        let text = self.text.downgrade();
        let weak = self.weak.clone();
        let pending = self.pending_heading.borrow_mut().take();
        let mark = pending
            .as_ref()
            .and_then(|s| self.anchors.borrow().get(s).cloned());
        glib::idle_add_local_once(move || {
            adjustment
                .set_value(previous.min((adjustment.upper() - adjustment.page_size()).max(0.0)));
            if let (Some(text), Some(mark)) = (text.upgrade(), mark) {
                text.scroll_to_mark(&mark, 0.1, true, 0.0, 0.0);
                if let Some(view) = weak.upgrade() {
                    view.range_jump_pending.set(false);
                }
            } else if let Some(view) = weak.upgrade()
                && view.range_jump_pending.replace(false)
            {
                view.scroll_poc_highlight();
            }
        });
        self.emit_ready();
    }

    fn emit_ready(&self) {
        if let Some(error) = self.read_error.borrow().clone() {
            (self.emit)(DocumentEvent::Failed(error));
            return;
        }
        if self.content.borrow().is_none() || self.job.borrow().is_some() {
            return;
        }
        if let Some(warning) = self.combined_warning() {
            (self.emit)(DocumentEvent::ReadyWithWarnings(self.title(), warning));
        } else {
            (self.emit)(DocumentEvent::Ready(self.title()));
        }
    }

    fn ready_status(&self) {
        if let Some(error) = self.read_error.borrow().as_ref() {
            self.status.set_text(&format!(
                "Could not update: {error} · last good content retained"
            ));
            return;
        }
        if self.content.borrow().is_none() && self.job.borrow().is_none() {
            self.status.set_text("Loading native Markdown…");
            return;
        }
        if self.job.borrow().is_some() {
            self.status
                .set_text("Rendering native diagrams… previous content retained");
            return;
        }
        if let Some(warning) = self.combined_warning() {
            self.status
                .set_text(&format!("Native Markdown · {warning}"));
            return;
        }
        self.status.set_text(if self.monitor.borrow().is_some() {
            "Native Markdown · watching disk"
        } else {
            "Native Markdown · live watch unavailable; use Reload"
        });
    }

    fn title(&self) -> Option<String> {
        self.source
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
    }

    fn failure(&self, error: &str) {
        *self.read_error.borrow_mut() = Some(error.to_owned());
        self.status.set_text(&format!(
            "Could not update: {error} · last good content retained"
        ));
        (self.emit)(DocumentEvent::Failed(error.to_owned()));
    }

    pub fn set_highlights(&self, ranges: Vec<smad_protocol::LineRange>) {
        self.range_entry
            .set_text(&smad_protocol::format_ranges(&ranges));
        *self.poc_ranges.borrow_mut() = ranges;
        self.range_cursor.set(0);
        self.range_jump_pending.set(true);
        if self.content.borrow().is_some() {
            self.apply_poc_highlights();
            self.scroll_poc_highlight();
            self.range_jump_pending.set(false);
        }
    }

    fn combined_warning(&self) -> Option<String> {
        let warnings: Vec<_> = self
            .warnings
            .borrow()
            .iter()
            .cloned()
            .chain(self.mapping_warning.borrow().iter().cloned())
            .collect();
        (!warnings.is_empty()).then(|| warnings.join(" · "))
    }

    fn apply_poc_highlights(&self) {
        let report = self
            .source_map
            .borrow()
            .apply(&self.text.buffer(), &self.poc_ranges.borrow());
        self.poc_status.set_text(
            report
                .as_deref()
                .unwrap_or("Source lines, not visual rows · diagrams/images select whole widgets"),
        );
        *self.mapping_warning.borrow_mut() = report;
    }

    fn scroll_poc_highlight(&self) {
        let map = self.source_map.borrow();
        let ranges = self.poc_ranges.borrow();
        let ranges = ranges.get(self.range_cursor.get()).into_iter();
        let intersects = |lines: &std::ops::Range<usize>| {
            ranges
                .clone()
                .any(|r| (r.start as usize) < lines.end && r.end as usize >= lines.start)
        };
        let text = map
            .spans
            .iter()
            .filter(|s| intersects(&s.lines))
            .map(|s| s.chars.start);
        let attachments = self.attachments.borrow();
        let widgets = map
            .widgets
            .iter()
            .filter(|(s, _)| intersects(s))
            .filter_map(|(_, widget)| {
                attachments
                    .iter()
                    .find(|item| widget == &item.widget || widget.is_ancestor(&item.widget))
                    .map(|item| {
                        self.text
                            .buffer()
                            .iter_at_child_anchor(&item.anchor)
                            .offset()
                    })
            });
        if let Some(offset) = text.chain(widgets).min() {
            self.text.scroll_to_iter(
                &mut self.text.buffer().iter_at_offset(offset),
                0.1,
                true,
                0.0,
                0.2,
            );
        }
    }

    pub fn heading(&self, fragment: &str) {
        // Decode URL fragments without treating a '+' as a space.
        let decoded = glib::uri_unescape_string(fragment, None::<&str>)
            .map(|s| s.to_string())
            .unwrap_or_else(|| fragment.to_owned());
        if let Some(mark) = self.anchors.borrow().get(&decoded) {
            self.text.scroll_to_mark(mark, 0.1, true, 0.0, 0.0);
        } else if self.content.borrow().is_none() {
            *self.pending_heading.borrow_mut() = Some(decoded);
        } else {
            self.status
                .set_text(&format!("Heading not found: {decoded}"));
        }
    }

    fn follow_link(&self, href: &str) {
        let result = markdown::resolve_link(&self.source, href).and_then(|target| match target {
            LinkTarget::Markdown { path, fragment } => {
                let canonical = markdown::canonical_markdown(&path)?;
                if canonical == self.source {
                    if let Some(fragment) = fragment {
                        self.heading(&fragment);
                    }
                    Ok(())
                } else {
                    (self.navigate)(
                        canonical.to_str().expect("validated UTF-8").into(),
                        fragment,
                    )
                }
            }
            LinkTarget::External(url) => {
                gio::AppInfo::launch_default_for_uri(&url, gio::AppLaunchContext::NONE)
                    .map_err(|e| e.to_string())
            }
        });
        if let Err(error) = result {
            self.status.set_text(&format!("Link not opened: {error}"));
        }
    }
}

impl Drop for MarkdownView {
    fn drop(&mut self) {
        if let Some((control, poll)) = self.job.get_mut().take() {
            control.cancel();
            poll.remove();
        }
        if let Some(timer) = self.debounce.get_mut().take() {
            timer.remove();
        }
        if let Some(monitor) = self.monitor.get_mut().take() {
            monitor.cancel();
        }
    }
}

struct Embedded {
    anchor: gtk::TextChildAnchor,
    widget: gtk::Widget,
    copy: String,
}

struct Rendered {
    source_map: SourceMap,
    attachments: Vec<Embedded>,
    diagram_errors: usize,
    buffer: gtk::TextBuffer,
    anchors: HashMap<String, gtk::TextMark>,
    links: Vec<(i32, i32, String)>,
}

fn insert(buffer: &gtk::TextBuffer, text: &str, tags: &[&str]) {
    buffer.insert_with_tags_by_name(&mut buffer.end_iter(), text, tags);
}

fn line_break(buffer: &gtk::TextBuffer) {
    let mut end = buffer.end_iter();
    if end.backward_char() && end.char() != '\n' {
        insert(buffer, "\n", &[]);
    }
}

fn render(
    content: &str,
    source: &Path,
    diagrams: &mermaid::Diagrams,
    follow: Rc<dyn Fn(&str)>,
) -> Result<Rendered, String> {
    if content.lines().count() > 20_000 {
        return Err("Markdown limit: 20,000 source lines".into());
    }
    let events: Vec<_> = Parser::new_ext(
        content,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    )
    .into_offset_iter()
    .take(20_001)
    .collect();
    if events.len() > 20_000 {
        return Err("Markdown POC limit: 20,000 parser events".into());
    }
    let buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
    let table = buffer.tag_table();
    table.add(&gtk::TextTag::builder().name("bold").weight(700).build());
    table.add(
        &gtk::TextTag::builder()
            .name("italic")
            .style(gtk::pango::Style::Italic)
            .build(),
    );
    table.add(
        &gtk::TextTag::builder()
            .name("strike")
            .strikethrough(true)
            .build(),
    );
    table.add(
        &gtk::TextTag::builder()
            .name("code")
            .family("monospace")
            .scale(0.9)
            .build(),
    );
    table.add(
        &gtk::TextTag::builder()
            .name("codeblock")
            .family("monospace")
            .scale(0.9)
            .left_margin(48)
            .pixels_above_lines(2)
            .pixels_below_lines(2)
            .build(),
    );
    table.add(
        &gtk::TextTag::builder()
            .name("quote")
            .left_margin(24)
            .style(gtk::pango::Style::Italic)
            .build(),
    );
    table.add(
        &gtk::TextTag::builder()
            .name("link")
            .underline(gtk::pango::Underline::Single)
            .foreground("#3584e4")
            .build(),
    );
    table.add(
        &gtk::TextTag::builder()
            .name("table")
            .family("monospace")
            .scale(0.85)
            .wrap_mode(gtk::WrapMode::None)
            .build(),
    );
    for (level, scale) in [(1, 1.9), (2, 1.5), (3, 1.25), (4, 1.1), (5, 1.0), (6, 1.0)] {
        table.add(
            &gtk::TextTag::builder()
                .name(format!("h{level}"))
                .weight(700)
                .scale(scale)
                .pixels_above_lines(18)
                .pixels_below_lines(10)
                .build(),
        );
    }
    let mut tags: Vec<&str> = Vec::new();
    let mut anchors = HashMap::new();
    let mut links = Vec::new();
    let mut link_start = None;
    let mut heading_start = None;
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut image: Option<(String, String)> = None;
    let mut image_count = 0;
    let mut active_table: Option<table::Table> = None;
    let mut code: Option<(String, String)> = None;
    let mut attachments = Vec::new();
    let mut diagram_errors = 0;
    let lines = Lines::new(content);
    let mut source_map = SourceMap::default();
    let mut code_parts = Vec::new();
    let mut code_chars = 0;
    let mut code_start = 0;
    let mut image_start = 0;
    let mut table_rows: Vec<std::ops::Range<usize>> = Vec::new();
    for (event, bytes) in events {
        let output_start = buffer.char_count();
        if let Some((_, text)) = code.as_mut() {
            match event {
                Event::Text(part) => {
                    code_parts.push((bytes.clone(), part.to_string(), code_chars));
                    code_chars += part.chars().count() as i32;
                    text.push_str(&part);
                }
                Event::End(TagEnd::CodeBlock) => {
                    let (language, text) = code.take().expect("code block");
                    if language == "mermaid" {
                        match diagrams
                            .get(&text)
                            .ok_or_else(|| "diagram was not prepared".to_owned())
                            .and_then(|svg| svg.as_ref().map_err(Clone::clone))
                            .and_then(|svg| diagram_widget(svg, &text))
                        {
                            Ok(widget) => {
                                source_map
                                    .widgets
                                    .push((lines.range(code_start..bytes.end), widget.clone()));
                                embed(
                                    &buffer,
                                    &mut attachments,
                                    widget,
                                    format!("```mermaid\n{text}```\n"),
                                );
                            }
                            Err(error) => {
                                diagram_errors += 1;
                                insert(
                                    &buffer,
                                    &format!("Diagram unavailable: {error}\n"),
                                    &["italic"],
                                );
                                let start = buffer.char_count();
                                highlighted_code(&buffer, &text, "");
                                for (range, text, offset) in &code_parts {
                                    source_map.text(
                                        &lines,
                                        content,
                                        range.clone(),
                                        text,
                                        start + offset,
                                    );
                                }
                            }
                        }
                    } else {
                        highlighted_code(&buffer, &text, &language);
                        for (range, text, offset) in &code_parts {
                            source_map.text(
                                &lines,
                                content,
                                range.clone(),
                                text,
                                output_start + offset,
                            );
                        }
                    }
                    code_parts.clear();
                    code_chars = 0;
                    insert(&buffer, "\n", &[]);
                }
                _ => {}
            }
            continue;
        }
        if let Some((_, alt)) = image.as_mut() {
            match event {
                Event::End(TagEnd::Image) => {
                    let (destination, alt) = image.take().expect("image");
                    image_count += 1;
                    if image_count <= 32 {
                        if let Some(texture) = local_image(source, &destination) {
                            let picture = gtk::Picture::for_paintable(&texture);
                            picture.set_can_shrink(false);
                            picture.set_alternative_text(Some(&alt));
                            let container = gtk::Box::new(gtk::Orientation::Vertical, 4);
                            container.append(&picture);
                            let caption = gtk::Label::new(Some(&alt));
                            caption.set_wrap(true);
                            container.append(&caption);
                            let widget: gtk::Widget = container.upcast();
                            source_map
                                .widgets
                                .push((lines.range(image_start..bytes.end), widget.clone()));
                            embed(&buffer, &mut attachments, widget, format!("\n{alt}\n"));
                        } else {
                            insert(&buffer, &format!("[Image unavailable: {alt}]"), &[]);
                        }
                    } else {
                        insert(&buffer, "[Image limit reached]", &[]);
                    }
                    source_map.atomic(
                        &lines,
                        image_start..bytes.end,
                        output_start..buffer.char_count(),
                    );
                }
                Event::Text(text) | Event::Code(text) => alt.push_str(&text),
                _ => {}
            }
            continue;
        }
        if let Some(table) = active_table.as_mut() {
            if matches!(event, Event::End(TagEnd::Table)) {
                let table = active_table.take().expect("active table");
                let (widget, plain) = table.widget(follow.clone())?;
                if links.len() + table.links.len() > 256 {
                    return Err("document limit: 256 links".into());
                }
                // Negative ranges represent native table-cell links, exposed in the Links menu.
                links.extend(table.links.into_iter().map(|uri| (-1, -1, uri)));
                if let Some(grid) = widget.downcast_ref::<gtk::Grid>() {
                    for (row, range) in table_rows.drain(..).enumerate() {
                        for column in 0..12 {
                            if let Some(cell) = grid.child_at(column, row as i32) {
                                source_map.widgets.push((lines.range(range.clone()), cell));
                            }
                        }
                    }
                }
                embed(&buffer, &mut attachments, widget, plain);
            } else {
                if matches!(event, Event::Start(Tag::TableHead | Tag::TableRow)) {
                    table_rows.push(bytes.clone());
                }
                table.push(event);
            }
            continue;
        }
        let mapping_text = match &event {
            Event::Text(text) | Event::Code(text) | Event::Html(text) | Event::InlineHtml(text) => {
                Some(text.to_string())
            }
            _ => None,
        };
        let inline_code = matches!(event, Event::Code(_));
        let atomic = matches!(
            event,
            Event::SoftBreak | Event::HardBreak | Event::Rule | Event::TaskListMarker(_)
        );
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { level, .. } => {
                    heading_start = Some(buffer.char_count());
                    tags.push(match level as u8 {
                        1 => "h1",
                        2 => "h2",
                        3 => "h3",
                        4 => "h4",
                        5 => "h5",
                        _ => "h6",
                    });
                }
                Tag::Strong => tags.push("bold"),
                Tag::Emphasis => tags.push("italic"),
                Tag::Strikethrough => tags.push("strike"),
                Tag::BlockQuote(_) => tags.push("quote"),
                Tag::CodeBlock(kind) => {
                    let language = match kind {
                        CodeBlockKind::Fenced(info) => {
                            info.split_whitespace().next().unwrap_or("").to_owned()
                        }
                        CodeBlockKind::Indented => String::new(),
                    };
                    code_start = bytes.start;
                    code = Some((language, String::new()));
                }
                Tag::List(start) => {
                    line_break(&buffer);
                    if lists.len() >= 16 {
                        return Err("POC list nesting limit: 16 levels".into());
                    }
                    lists.push(start);
                }
                Tag::Item => {
                    line_break(&buffer);
                    insert(&buffer, &"  ".repeat(lists.len().saturating_sub(1)), &[]);
                    let marker = if let Some(Some(number)) = lists.last_mut() {
                        let marker = format!("{number}. ");
                        *number = number.saturating_add(1);
                        marker
                    } else {
                        "• ".into()
                    };
                    insert(&buffer, &marker, &tags);
                    source_map.atomic(
                        &lines,
                        bytes.start..bytes.start + 1,
                        output_start..buffer.char_count(),
                    );
                }
                Tag::Link { dest_url, .. } => {
                    link_start = Some((buffer.char_count(), dest_url.to_string()));
                    tags.push("link");
                }
                Tag::Image { dest_url, .. } => {
                    image_start = bytes.start;
                    image = Some((dest_url.to_string(), String::new()));
                }
                Tag::Table(alignments) => active_table = Some(table::Table::new(alignments)),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    if let Some(start) = heading_start.take() {
                        let text =
                            buffer.text(&buffer.iter_at_offset(start), &buffer.end_iter(), false);
                        let slug = markdown::heading_slug(&text);
                        let mut unique = slug.clone();
                        let mut count = 1;
                        while anchors.contains_key(&unique) {
                            unique = format!("{slug}-{count}");
                            count += 1;
                        }
                        anchors.insert(
                            unique,
                            buffer.create_mark(None, &buffer.iter_at_offset(start), true),
                        );
                    }
                    tags.pop();
                    insert(&buffer, "\n", &[]);
                }
                TagEnd::Paragraph => {
                    line_break(&buffer);
                    if lists.is_empty() {
                        insert(&buffer, "\n", &[]);
                    }
                }
                TagEnd::Strong
                | TagEnd::Emphasis
                | TagEnd::Strikethrough
                | TagEnd::BlockQuote(_) => {
                    tags.pop();
                }
                TagEnd::List(_) => {
                    lists.pop();
                    line_break(&buffer);
                    if lists.is_empty() {
                        insert(&buffer, "\n", &[]);
                    }
                }
                TagEnd::Item => line_break(&buffer),
                TagEnd::Link => {
                    tags.pop();
                    if let Some((start, href)) = link_start.take() {
                        if links.len() >= 256 {
                            return Err("POC link limit: 256 links".into());
                        }
                        links.push((start, buffer.char_count(), href));
                    }
                }
                _ => {}
            },
            Event::Text(text) => insert(&buffer, &text, &tags),
            Event::Code(text) => {
                let mut inline = tags.clone();
                inline.push("code");
                insert(&buffer, &text, &inline);
            }
            Event::SoftBreak => insert(&buffer, " ", &tags),
            Event::HardBreak => insert(&buffer, "\n", &tags),
            Event::Rule => insert(&buffer, "────────────────────────\n\n", &[]),
            Event::TaskListMarker(checked) => {
                insert(&buffer, if checked { "☑ " } else { "☐ " }, &tags)
            }
            // Raw HTML is inert text, never evaluated by a browser.
            Event::Html(text) | Event::InlineHtml(text) => insert(&buffer, &text, &["code"]),
            _ => {}
        }
        if let Some(text) = mapping_text {
            if inline_code {
                source_map.inline_code(&lines, content, bytes, &text, output_start);
            } else {
                source_map.text(&lines, content, bytes, &text, output_start);
            }
        } else if atomic {
            source_map.atomic(&lines, bytes, output_start..buffer.char_count());
        }
    }
    Ok(Rendered {
        source_map,
        attachments,
        diagram_errors,
        buffer,
        anchors,
        links,
    })
}

fn embed(
    buffer: &gtk::TextBuffer,
    attachments: &mut Vec<Embedded>,
    widget: gtk::Widget,
    copy: String,
) {
    line_break(buffer);
    let anchor = buffer.create_child_anchor(&mut buffer.end_iter());
    attachments.push(Embedded {
        anchor,
        widget,
        copy,
    });
    insert(buffer, "\n\n", &[]);
}

fn highlighted_code(buffer: &gtk::TextBuffer, source: &str, language: &str) {
    let dark = gtk::Settings::default()
        .is_some_and(|settings| settings.is_gtk_application_prefer_dark_theme());
    for (style, text) in highlight::code_spans(source, language, dark) {
        if let Some(style) = style {
            let color = style.foreground;
            let name = format!(
                "syntax-{:02x}{:02x}{:02x}-{}",
                color.r,
                color.g,
                color.b,
                style.font_style.bits()
            );
            if buffer.tag_table().lookup(&name).is_none() {
                let tag = gtk::TextTag::builder()
                    .name(&name)
                    .foreground(format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b))
                    .weight(
                        if style
                            .font_style
                            .contains(syntect::highlighting::FontStyle::BOLD)
                        {
                            700
                        } else {
                            400
                        },
                    )
                    .style(
                        if style
                            .font_style
                            .contains(syntect::highlighting::FontStyle::ITALIC)
                        {
                            gtk::pango::Style::Italic
                        } else {
                            gtk::pango::Style::Normal
                        },
                    )
                    .build();
                buffer.tag_table().add(&tag);
            }
            insert(buffer, text, &["codeblock", &name]);
        } else {
            insert(buffer, text, &["codeblock"]);
        }
    }
}

fn diagram_widget(svg: &str, source: &str) -> Result<gtk::Widget, String> {
    let loader = gtk::gdk_pixbuf::PixbufLoader::with_type("svg").map_err(|e| e.to_string())?;
    loader.connect_size_prepared(|loader, width, height| {
        let scale = (1000.0 / width.max(1) as f64)
            .min(1000.0 / height.max(1) as f64)
            .min(1.0);
        loader.set_size(
            (width as f64 * scale).max(1.0) as i32,
            (height as f64 * scale).max(1.0) as i32,
        );
    });
    loader.write(svg.as_bytes()).map_err(|e| e.to_string())?;
    loader.close().map_err(|e| e.to_string())?;
    let pixbuf = loader
        .pixbuf()
        .ok_or("native SVG decoder produced no image")?;
    let texture = gdk::Texture::for_pixbuf(&pixbuf);
    let picture = gtk::Picture::for_paintable(&texture);
    picture.set_can_shrink(false);
    picture.set_alternative_text(Some("Native Mermaid diagram"));
    let container = gtk::Box::new(gtk::Orientation::Vertical, 8);
    container.append(&picture);
    let source_label = gtk::Label::new(Some(source));
    source_label.set_selectable(true);
    source_label.set_xalign(0.0);
    source_label.add_css_class("monospace");
    let expander = gtk::Expander::builder()
        .label("Mermaid source · native rendering")
        .child(&source_label)
        .build();
    container.append(&expander);
    Ok(container.upcast())
}

fn local_image(source: &Path, destination: &str) -> Option<gdk::Texture> {
    let base = url::Url::from_file_path(source).ok()?;
    let path = base.join(destination).ok()?.to_file_path().ok()?;
    if std::fs::metadata(&path).ok()?.len() > 8 * 1024 * 1024 {
        return None;
    }
    let (_, width, height) = gtk::gdk_pixbuf::Pixbuf::file_info(&path)?;
    if width > 8192 || height > 8192 {
        return None;
    }
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, 720, 480, true).ok()?;
    Some(gdk::Texture::for_pixbuf(&pixbuf))
}

#[cfg(test)]
#[path = "markdown_mapping_tests.rs"]
mod mapping_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a display; run under isolated Xvfb with --ignored --test-threads=1"]
    fn native_reader_live_updates_selection_links_and_cleanup() {
        gtk::init().expect("GTK display");
        gtk::Settings::default()
            .unwrap()
            .set_gtk_enable_animations(false);
        let directory =
            std::env::temp_dir().join(format!("lavish-native-reader-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("reader.md");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/markdown");
        for file in ["reader.md", "next.md", "landscape.svg"] {
            std::fs::copy(fixture.join(file), directory.join(file)).unwrap();
        }
        let events = Rc::new(RefCell::new(Vec::new()));
        let captured = events.clone();
        let navigations = Rc::new(RefCell::new(Vec::new()));
        let routed = navigations.clone();
        let view = MarkdownView::new(
            &source,
            "native-test",
            Rc::new(move |event| captured.borrow_mut().push(event)),
            Rc::new(move |path, fragment| {
                routed.borrow_mut().push((path, fragment));
                Ok(())
            }),
        );
        let window = gtk::Window::builder()
            .default_width(920)
            .default_height(760)
            .child(&view.root)
            .build();
        window.present();
        let pump = |milliseconds| {
            let deadline = std::time::Instant::now() + Duration::from_millis(milliseconds);
            while std::time::Instant::now() < deadline {
                while glib::MainContext::default().pending() {
                    glib::MainContext::default().iteration(false);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        for _ in 0..100 {
            pump(50);
            if view.content.borrow().is_some() {
                break;
            }
        }
        assert!(matches!(
            events.borrow().last(),
            Some(DocumentEvent::Ready(_))
        ));
        let original = view.content.borrow().clone().unwrap();
        let buffer = view.text.buffer();
        buffer.select_range(&buffer.start_iter(), &buffer.end_iter());
        let (start, end) = buffer.selection_bounds().unwrap();
        let selected = view.selection_text(&start, &end);
        assert!(
            view.attachments
                .borrow()
                .iter()
                .any(|item| item.widget.is::<gtk::Grid>())
        );
        assert!(
            view.attachments
                .borrow()
                .iter()
                .any(|item| item.copy.starts_with("```mermaid"))
        );
        assert!(
            buffer.tag_table().size() > 16,
            "syntax highlight tags were not generated"
        );
        assert!(selected.contains("A quiet place"));
        assert!(selected.contains("println!"));
        assert!(selected.contains("Existing WebKit"));
        assert!(selected.contains("<script>"));
        assert!(view.anchors.borrow().contains_key("a-small-plan-1"));
        assert!(local_image(&source, "landscape.svg").is_some());
        view.link_menu.popup();
        pump(50);
        let popup = view.link_menu.popover().unwrap();
        let scroll = popup
            .child()
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap();
        let list = scroll
            .child()
            .unwrap()
            .downcast::<gtk::Viewport>()
            .unwrap()
            .child()
            .unwrap();
        let mut child = list.first_child();
        let mut clicked = false;
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Ok(button) = widget.downcast::<gtk::Button>()
                && button.label().as_deref() == Some("next.md#next-steps")
            {
                button.emit_clicked();
                clicked = true;
                break;
            }
        }
        assert!(
            clicked,
            "relative link is keyboard-accessible in Links menu"
        );
        assert_eq!(
            navigations.borrow()[0],
            (
                directory.join("next.md").to_str().unwrap().into(),
                Some("next-steps".into())
            )
        );
        view.follow_link("unsupported.html");
        assert_eq!(navigations.borrow().len(), 1);
        assert!(view.status.text().contains("Link not opened"));
        buffer.place_cursor(&buffer.start_iter());
        view.heading("native-table");
        pump(100);
        view.text
            .scroll_to_iter(&mut buffer.start_iter(), 0.0, true, 0.0, 0.0);
        pump(100);
        view.scroll.vadjustment().set_value(0.0);
        view.status.set_text("Native Markdown · watching disk");
        pump(100);
        if let Ok(path) = std::env::var("SMAD_MARKDOWN_SCREENSHOT") {
            let snapshot = gtk::Snapshot::new();
            let paintable = gtk::WidgetPaintable::new(Some(&window));
            paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
            let node = snapshot.to_node().expect("visible reader snapshot");
            window
                .renderer()
                .unwrap()
                .render_texture(&node, None)
                .save_to_png(path)
                .unwrap();
        }
        let rss = || {
            std::fs::read_to_string("/proc/self/status")
                .unwrap()
                .lines()
                .find(|line| line.starts_with("VmRSS:"))
                .unwrap()
                .to_owned()
        };
        let initial_rss = rss();
        view.scroll.vadjustment().set_value(180.0);
        pump(50);
        let saved_scroll = view.scroll.vadjustment().value();
        let replacement = directory.join("replacement.md");
        std::fs::write(
            &replacement,
            format!("{original}\nAtomic replacement sentinel\n"),
        )
        .unwrap();
        std::fs::rename(&replacement, &source).unwrap();
        pump(450);
        assert!(
            view.content
                .borrow()
                .as_ref()
                .unwrap()
                .contains("Atomic replacement sentinel")
        );
        assert!(
            (view.scroll.vadjustment().value() - saved_scroll).abs() < 2.0,
            "live update changed scroll position"
        );
        let last_good = view.text.buffer();
        std::fs::remove_file(&source).unwrap();
        pump(450);
        assert!(matches!(
            events.borrow().last(),
            Some(DocumentEvent::Failed(_))
        ));
        assert_eq!(last_good, view.text.buffer());
        std::fs::write(&source, &original).unwrap();
        pump(450);
        assert!(matches!(
            events.borrow().last(),
            Some(DocumentEvent::Ready(_))
        ));
        for index in 0..30 {
            std::fs::write(&source, format!("{original}\nUpdate {index}\n")).unwrap();
            view.reload();
            for _ in 0..100 {
                pump(20);
                if view
                    .content
                    .borrow()
                    .as_ref()
                    .is_some_and(|text| text.contains(&format!("Update {index}")))
                {
                    break;
                }
            }
            assert!(
                view.content
                    .borrow()
                    .as_ref()
                    .is_some_and(|text| text.contains(&format!("Update {index}")))
            );
        }
        println!(
            "Native GTK RSS after initial render: {initial_rss}; after 30 updates: {}",
            rss()
        );
        // A superseded background diagram result must never replace newer text.
        std::fs::write(&source, "```mermaid\nflowchart LR\n A --> B\n```\n").unwrap();
        view.reload();
        std::fs::write(&source, "# Newest revision\n").unwrap();
        view.reload();
        pump(300);
        assert_eq!(
            view.content.borrow().as_deref(),
            Some("# Newest revision\n")
        );
        let features = std::fs::read_to_string(fixture.join("features.md")).unwrap();
        std::fs::write(&source, &features).unwrap();
        view.reload();
        for _ in 0..100 {
            pump(50);
            if view.content.borrow().as_ref() == Some(&features) {
                break;
            }
        }
        assert_eq!(view.content.borrow().as_ref(), Some(&features));
        pump(100);
        let grid_width = view
            .attachments
            .borrow()
            .iter()
            .find(|item| item.widget.is::<gtk::Grid>())
            .unwrap()
            .widget
            .width();
        assert!(
            grid_width >= 300,
            "native table was compressed to unreadable columns: {grid_width}"
        );
        assert!(view.status.text().contains("1 diagram error"));
        assert_eq!(
            view.attachments
                .borrow()
                .iter()
                .filter(|item| item.copy.starts_with("```mermaid"))
                .count(),
            2
        );
        if let Ok(path) = std::env::var("SMAD_MARKDOWN_SCREENSHOT") {
            view.scroll.vadjustment().set_value(0.0);
            pump(100);
            let snapshot = gtk::Snapshot::new();
            gtk::WidgetPaintable::new(Some(&window)).snapshot(
                &snapshot,
                window.width() as f64,
                window.height() as f64,
            );
            window
                .renderer()
                .unwrap()
                .render_texture(snapshot.to_node().unwrap(), None)
                .save_to_png(Path::new(&path).with_file_name("renderer-features.png"))
                .unwrap();
            view.heading("a-native-diagram");
            pump(150);
            let snapshot = gtk::Snapshot::new();
            gtk::WidgetPaintable::new(Some(&window)).snapshot(
                &snapshot,
                window.width() as f64,
                window.height() as f64,
            );
            window
                .renderer()
                .unwrap()
                .render_texture(snapshot.to_node().unwrap(), None)
                .save_to_png(Path::new(&path).with_file_name("mermaid-diagrams.png"))
                .unwrap();
        }
        let weak = Rc::downgrade(&view);
        window.set_child(None::<&gtk::Widget>);
        drop(view);
        assert!(weak.upgrade().is_none(), "view/watcher cycle leaked");
        window.destroy();
        pump(250);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
