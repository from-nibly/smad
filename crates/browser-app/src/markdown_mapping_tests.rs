use super::*;

fn highlighted(buffer: &gtk::TextBuffer) -> String {
    let tag = buffer.tag_table().lookup("source-poc-highlight").unwrap();
    (0..buffer.char_count())
        .filter_map(|offset| {
            let iter = buffer.iter_at_offset(offset);
            iter.has_tag(&tag).then(|| iter.char())
        })
        .collect()
}

#[test]
#[ignore = "requires private display; one GTK test per process"]
fn source_line_mapping_poc() {
    gtk::init().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/markdown/source-mapping.md");
    let content = std::fs::read_to_string(&fixture).unwrap();
    // Use the real diagram renderer, not a placeholder widget.
    let (_control, result) = mermaid::prepare(&content).unwrap();
    let diagrams = result
        .recv_timeout(Duration::from_secs(30))
        .unwrap()
        .unwrap();
    let rendered = render(&content, &fixture, &diagrams, Rc::new(|_| {})).unwrap();
    assert_eq!(rendered.diagram_errors, 0);
    assert!(
        rendered.source_map.ambiguous.is_empty(),
        "{:?}",
        rendered.source_map.ambiguous
    );
    let line = |needle: &str| content.lines().position(|s| s.contains(needle)).unwrap() as u32 + 1;
    let apply = |needle: &str| {
        let line = line(needle);
        rendered.source_map.apply(
            &rendered.buffer,
            &[smad_protocol::LineRange {
                start: line,
                end: line,
            }],
        );
        highlighted(&rendered.buffer)
    };
    assert_eq!(
        apply("SECOND source").trim(),
        "SECOND source line: highlight only this text, including é, 🦀, &, and an escaped *star*."
    );
    assert_eq!(
        apply("HIGHLIGHT ONLY").trim_end(),
        "    let message = \"HIGHLIGHT ONLY THIS CODE LINE 🦀\";"
    );
    assert_eq!(
        apply("second indented").trim_end(),
        "second indented code line"
    );
    let blank = line("println!") + 1;
    let warning = rendered
        .source_map
        .apply(
            &rendered.buffer,
            &[smad_protocol::LineRange {
                start: blank,
                end: blank,
            }],
        )
        .unwrap();
    assert!(warning.contains(&format!(
        "blank rendered lines without visible markers (first 16): [{blank}]"
    )));
    assert_eq!(highlighted(&rendered.buffer), "\n");
    assert!(
        render(
            &"x\n".repeat(20_001),
            &fixture,
            &Default::default(),
            Rc::new(|_| {})
        )
        .err()
        .unwrap()
        .contains("20,000 source lines")
    );
    assert_eq!(apply("second = 2").trim_end(), "second = 2");
    assert_eq!(apply("second inline").trim(), "second inline");
    assert_eq!(
        apply("STRONG TARGET").trim(),
        "STRONG TARGET continues on this source line,"
    );
    assert_eq!(
        apply("UNKNOWN TARGET").trim(),
        "UNKNOWN TARGET stays precisely mapped."
    );
    assert_eq!(
        apply("Second quoted").trim(),
        "Second quoted source line with emphasis."
    );
    assert_eq!(
        apply("nested continuation").trim(),
        "nested continuation on a separate line."
    );
    apply("SELECT THIS ROW");
    let selected: Vec<_> = rendered
        .source_map
        .widgets
        .iter()
        .filter(|(_, w)| w.has_css_class("source-poc-highlight"))
        .collect();
    assert_eq!(selected.len(), 3);
    assert!(selected.iter().any(|(_, w)| {
        w.downcast_ref::<gtk::Label>()
            .is_some_and(|l| l.text() == "SELECT THIS ROW ONLY")
    }));
    apply("B --> C");
    let selected: Vec<_> = rendered
        .source_map
        .widgets
        .iter()
        .filter(|(_, w)| w.has_css_class("source-poc-highlight"))
        .collect();
    assert_eq!(selected.len(), 1);
    assert!(selected[0].1.is::<gtk::Box>());
    apply("![Local");
    assert_eq!(
        rendered
            .source_map
            .widgets
            .iter()
            .filter(|(_, w)| w.has_css_class("source-poc-highlight"))
            .count(),
        1
    );
    for needle in ["[target]:", "| :---", "-------------------", "```rust"] {
        assert!(apply(needle).is_empty(), "syntax-only line {needle}");
        assert!(
            !rendered
                .source_map
                .widgets
                .iter()
                .any(|(_, w)| w.has_css_class("source-poc-highlight"))
        );
    }
    // Assert the offset conversion against actual parser events, not only synthetic spans.
    let crlf = "> first é\r\n> SECOND 🦀 &amp;\r\n> third\r\n\r\n```rs\r\nlet first = 1;\r\nlet second = 2;\r\n```\r\n";
    let r = render(crlf, &fixture, &Default::default(), Rc::new(|_| {})).unwrap();
    r.source_map
        .apply(&r.buffer, &[smad_protocol::LineRange { start: 2, end: 2 }]);
    assert_eq!(highlighted(&r.buffer).trim(), "SECOND 🦀 &");
    r.source_map
        .apply(&r.buffer, &[smad_protocol::LineRange { start: 7, end: 7 }]);
    assert_eq!(highlighted(&r.buffer).trim(), "let second = 2;");

    let broken = "```mermaid\nBROKEN DIAGRAM\n```\n";
    let diagrams = [(
        "BROKEN DIAGRAM\n".to_owned(),
        Err("intentional POC diagnostic".to_owned()),
    )]
    .into_iter()
    .collect();
    let fallback = render(broken, &fixture, &diagrams, Rc::new(|_| {})).unwrap();
    assert_eq!(fallback.diagram_errors, 1);
    fallback.source_map.apply(
        &fallback.buffer,
        &[smad_protocol::LineRange { start: 2, end: 2 }],
    );
    assert_eq!(highlighted(&fallback.buffer).trim(), "BROKEN DIAGRAM");

    let text = gtk::TextView::with_buffer(&rendered.buffer);
    text.set_wrap_mode(gtk::WrapMode::WordChar);
    text.set_editable(false);
    text.set_left_margin(24);
    text.set_right_margin(24);
    for item in &rendered.attachments {
        text.add_child_at_anchor(&item.widget, &item.anchor);
    }
    let scroll = gtk::ScrolledWindow::builder().child(&text).build();
    let window = gtk::Window::builder()
        .default_width(1000)
        .default_height(800)
        .child(&scroll)
        .build();
    window.present();
    let pump = || {
        let end = std::time::Instant::now() + Duration::from_millis(250);
        while std::time::Instant::now() < end {
            while glib::MainContext::default().pending() {
                glib::MainContext::default().iteration(false);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    pump();
    if let Ok(path) = std::env::var("SMAD_MARKDOWN_SCREENSHOT") {
        for (name, needles, anchor) in [
            (
                "paragraph",
                vec!["SECOND source", "second inline"],
                "Paragraphs, wrapping, and inline formatting",
            ),
            (
                "code",
                vec!["HIGHLIGHT ONLY", "second indented", "second = 2"],
                "Exact code lines",
            ),
            ("widgets", vec!["SELECT THIS ROW", "B --> C"], "Tables"),
            ("images", vec!["![Local"], "Images and references"),
        ] {
            let mut ranges: Vec<_> = needles
                .iter()
                .map(|needle| smad_protocol::LineRange {
                    start: line(needle),
                    end: line(needle),
                })
                .collect();
            if name == "code" {
                ranges.push(smad_protocol::LineRange {
                    start: blank,
                    end: blank,
                });
            }
            rendered.source_map.apply(&rendered.buffer, &ranges);
            let mark = &rendered.anchors[&markdown::heading_slug(anchor)];
            text.scroll_to_mark(mark, 0.0, true, 0.0, 0.0);
            pump();
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
                .save_to_png(Path::new(&path).with_file_name(format!("mapping-{name}.png")))
                .unwrap();
        }
    }
    rendered.source_map.apply(&rendered.buffer, &[]);
    assert!(highlighted(&rendered.buffer).is_empty());
    assert!(
        !rendered
            .source_map
            .widgets
            .iter()
            .any(|(_, w)| w.has_css_class("source-poc-highlight"))
    );
    window.close();

    if std::env::var_os("SMAD_MARKDOWN_MAPPING_POC").is_some() {
        let directory =
            std::env::temp_dir().join(format!("smad-mapping-live-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("live.md");
        std::fs::write(&source, "alpha\nTARGET\nomega\n").unwrap();
        let events = Rc::new(RefCell::new(Vec::new()));
        let captured = events.clone();
        let view = MarkdownView::new(
            &source,
            "mapping-live",
            Rc::new(move |event| captured.borrow_mut().push(event)),
            Rc::new(|_, _| Ok(())),
        );
        *view.poc_ranges.borrow_mut() = vec![smad_protocol::LineRange { start: 2, end: 2 }];
        for _ in 0..40 {
            pump();
            if view.content.borrow().is_some() {
                break;
            }
        }
        assert_eq!(highlighted(&view.text.buffer()).trim(), "TARGET");
        let updated = "INTRODUCTION\nalpha\nTARGET\nomega\n";
        std::fs::write(directory.join("replacement"), updated).unwrap();
        std::fs::rename(directory.join("replacement"), &source).unwrap();
        for _ in 0..40 {
            pump();
            if view.content.borrow().as_deref() == Some(updated) {
                break;
            }
        }
        assert_eq!(view.content.borrow().as_deref(), Some(updated));
        assert_eq!(highlighted(&view.text.buffer()).trim(), "alpha");
        view.range_entry.set_text("");
        view.range_entry.emit_activate();
        assert!(highlighted(&view.text.buffer()).is_empty());
        assert!(events.borrow().iter().any(
            |event| matches!(event, DocumentEvent::HighlightsChanged(ranges) if ranges.is_empty())
        ));
        std::fs::remove_file(&source).unwrap();
        for _ in 0..40 {
            pump();
            if view.read_error.borrow().is_some() {
                break;
            }
        }
        assert!(view.read_error.borrow().is_some());
        view.range_entry.set_text("2");
        view.range_entry.emit_activate();
        assert!(
            matches!(events.borrow().last(), Some(DocumentEvent::Failed(_))),
            "range changes must not hide read failures"
        );
        std::fs::write(&source, updated).unwrap();
        for _ in 0..40 {
            pump();
            if view.read_error.borrow().is_none() {
                break;
            }
        }
        assert!(matches!(
            events.borrow().last(),
            Some(DocumentEvent::Ready(_))
        ));
        assert_eq!(highlighted(&view.text.buffer()).trim(), "alpha");
        drop(view);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
