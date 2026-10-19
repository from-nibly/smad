use super::*;
use smad_core::BrowserModel;
use smad_protocol::{DocumentKind, ProjectKey, ProjectMetadata};
use std::{path::Path, time::Instant};

fn pump(millis: u64) {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + Duration::from_millis(millis);
    while Instant::now() < deadline {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn wait(check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !check() {
        assert!(Instant::now() < deadline, "native source test timed out");
        pump(25);
    }
    pump(100);
}
fn capture(window: &gtk::Window, name: &str) {
    if let Ok(path) = std::env::var("SMAD_MARKDOWN_SCREENSHOT") {
        pump(100);
        let snapshot = gtk::Snapshot::new();
        gtk::WidgetPaintable::new(Some(window)).snapshot(
            &snapshot,
            window.width() as f64,
            window.height() as f64,
        );
        window
            .renderer()
            .unwrap()
            .render_texture(snapshot.to_node().unwrap(), None)
            .save_to_png(Path::new(&path).with_file_name(name))
            .unwrap();
    }
}
fn click(view: &SourceView, label: &str) {
    let toolbar = view.root.first_child().unwrap().next_sibling().unwrap();
    let mut child = toolbar.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Ok(button) = widget.downcast::<gtk::Button>()
            && button.label().as_deref() == Some(label)
        {
            button.emit_clicked();
            return;
        }
    }
    panic!("missing button {label}");
}

#[test]
#[ignore = "requires the isolated native GTK harness"]
fn whole_source_comparison_ranges_live_updates_and_cleanup() {
    gtk::init().unwrap();
    let directory = PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap())
        .join(format!("source-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("example.rs");
    let original = format!(
        "// UTF-8 é🦀 — whole-file source\nfn main() {{\n{}\n}}",
        (1..=130)
            .map(|line| if line == 2 {
                String::new()
            } else {
                format!(
                    "    let value_{line} = {line}; // original source line {}",
                    line + 2
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    );
    std::fs::write(&path, &original).unwrap();
    let project = ProjectMetadata {
        key: ProjectKey::Standalone {
            label: "Source test".into(),
        },
        label: "Source test".into(),
        raw_tab_name: None,
    };
    let mut model = BrowserModel::default();
    model.open_document(
        project,
        path.to_str().unwrap().into(),
        String::new(),
        DocumentKind::Code,
        1,
    );
    let mut document = model.projects[0].documents[0].clone();
    document.highlights = vec![
        LineRange { start: 2, end: 4 },
        LineRange { start: 90, end: 92 },
    ];
    let window = gtk::Window::builder()
        .title("SMAD native source validation")
        .default_width(1180)
        .default_height(820)
        .build();
    for dark in [false, true] {
        gtk::Settings::default()
            .unwrap()
            .set_gtk_application_prefer_dark_theme(dark);
        let view = SourceView::new(&document, "source-validation", Rc::new(|_| {}));
        window.set_child(Some(&view.widget()));
        window.present();
        wait(|| view.rendered.borrow().is_some());
        let buffer = view.text.buffer();
        assert_eq!(
            buffer.text(&buffer.start_iter(), &buffer.end_iter(), true),
            original
        );
        let tag = buffer.tag_table().lookup("agent-highlight").unwrap();
        assert!(tag.property::<bool>("paragraph-background-set"));
        assert!(!tag.property::<bool>("underline-set"));
        assert_eq!(tag.priority(), buffer.tag_table().size() - 1);
        {
            let rendered = view.rendered.borrow();
            let rendered = rendered.as_ref().unwrap();
            assert_eq!(rendered.rows.len(), original.lines().count());
            assert!(buffer.iter_at_offset(rendered.rows[1].start).has_tag(&tag));
            let blank = &rendered.rows[3];
            assert_eq!(
                buffer.text(
                    &buffer.iter_at_offset(blank.start),
                    &buffer.iter_at_offset(blank.end),
                    true
                ),
                "\n"
            );
            assert!(buffer.iter_at_offset(blank.start).has_tag(&tag));
            assert!(!buffer.iter_at_offset(rendered.rows[5].start).has_tag(&tag));
            assert!(buffer.iter_at_offset(rendered.rows[89].start).has_tag(&tag));
            assert!(
                rendered
                    .paints
                    .iter()
                    .any(|paint| paint.color != rendered.paints[0].color)
            );
        }
        capture(
            &window,
            if dark {
                "source-code-dark.png"
            } else {
                "source-code-light.png"
            },
        );
        // TextTag assertions alone miss GTK's empty-paragraph painting gap.
        // Sample the blank highlighted row near the viewport's right edge.
        let visible = view.text.visible_rect();
        let (y, height) = view.text.line_yrange(&buffer.iter_at_line(3).unwrap());
        let (x, y) = view.text.buffer_to_window_coords(
            gtk::TextWindowType::Widget,
            visible.x() + visible.width() - 8,
            y + height / 2,
        );
        let point = view
            .text
            .compute_point(&window, &gtk::graphene::Point::new(x as f32, y as f32))
            .unwrap();
        let snapshot = gtk::Snapshot::new();
        gtk::WidgetPaintable::new(Some(&window)).snapshot(
            &snapshot,
            window.width() as f64,
            window.height() as f64,
        );
        let texture = window
            .renderer()
            .unwrap()
            .render_texture(snapshot.to_node().unwrap(), None);
        let stride = texture.width() as usize * 4;
        let mut pixels = vec![0; stride * texture.height() as usize];
        texture.download(&mut pixels, stride);
        let offset = point.y() as usize * stride + point.x() as usize * 4;
        let native = u32::from_ne_bytes(pixels[offset..offset + 4].try_into().unwrap());
        assert_eq!(
            native,
            if dark { 0xff4b3d1e } else { 0xffffefb8 },
            "blank source row must visibly fill the viewport"
        );
        click(&view, "Next highlight");
        pump(180);
        assert!(view.scroll.vadjustment().value() > 0.0);
        click(&view, "Previous highlight");
        pump(180);
        view.highlights.borrow_mut().push(LineRange {
            start: 999,
            end: 1000,
        });
        view.reload();
        wait(|| view.status.text().contains("highlights exceed"));
        buffer.select_range(&buffer.start_iter(), &buffer.end_iter());
        assert_eq!(
            buffer.text(
                &buffer.selection_bounds().unwrap().0,
                &buffer.selection_bounds().unwrap().1,
                true
            ),
            original,
            "gutter numbers never enter selected text"
        );
        click(&view, "Clear highlights");
        assert!(view.targets(false).is_empty());
        assert!(!view.status.text().contains("highlights exceed"));
        pump(80);
        assert!(
            buffer.selection_bounds().is_some(),
            "clearing agent overlays must not clear text selection"
        );
        let weak = Rc::downgrade(&view);
        window.set_child(gtk::Widget::NONE);
        drop(view);
        pump(80);
        assert!(weak.upgrade().is_none());
    }
    gtk::Settings::default()
        .unwrap()
        .set_gtk_application_prefer_dark_theme(false);
    let store = SnapshotStore::from_environment().unwrap();
    let before = store.capture(&path).unwrap();
    let changed = original
        .replace("let value_1 = 1;", "let value_1 = 200; // modified")
        .replace("    let value_10 = 10; // original source line 12\n", "")
        .replace("let value_20 = 20;", "let value_20 = 2000;");
    std::fs::write(&path, &changed).unwrap();
    document.kind = DocumentKind::Comparison;
    document.key.kind = DocumentKind::Comparison;
    document.highlights = vec![
        LineRange { start: 3, end: 4 },
        LineRange { start: 21, end: 23 },
    ];
    document.old_highlights = vec![LineRange { start: 12, end: 12 }];
    document.comparison = Some(ComparisonSpec {
        before: Some(before.clone()),
        after: Some(store.capture(&path).unwrap()),
        live_after: Some(path.to_str().unwrap().into()),
        before_label: "Pinned original".into(),
        after_label: "Working tree".into(),
    });
    let view = SourceView::new(&document, "comparison-validation", Rc::new(|_| {}));
    window.set_child(Some(&view.widget()));
    wait(|| view.rendered.borrow().is_some());
    {
        let rendered = view.rendered.borrow();
        let rendered = rendered.as_ref().unwrap();
        assert_eq!(rendered.changes.len(), 3);
        assert!(
            rendered
                .rows
                .iter()
                .any(|row| row.coordinate.old == Some(12)
                    && row.coordinate.kind == RowKind::Removed
                    && view.marked(row))
        );
        assert!(
            rendered
                .rows
                .iter()
                .any(|row| row.coordinate.old == Some(133) && row.coordinate.new == Some(132)),
            "full trailing context retained with independent old/new coordinates"
        );
    }
    capture(&window, "source-comparison-light.png");
    click(&view, "Next change");
    click(&view, "Next change");
    click(&view, "Previous change");
    pump(100);
    view.scroll.vadjustment().set_value(900.0);
    pump(100);
    let scroll = view.scroll.vadjustment().value();
    let replacement = directory.join("replacement.rs");
    let updated = changed.replace(
        "let value_40 = 40;",
        "let value_40 = 400; // changed_after_atomic",
    );
    std::fs::write(&replacement, &updated).unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    wait(|| {
        view.rendered
            .borrow()
            .as_ref()
            .is_some_and(|rendered| rendered.text.contains("changed_after_atomic"))
    });
    assert!(
        (view.scroll.vadjustment().value() - scroll).abs() < 8.0,
        "live update preserves scroll"
    );
    assert_eq!(
        store.read(&before).unwrap().text,
        original,
        "base remains immutable"
    );
    std::fs::write(&path, [0_u8, 255]).unwrap();
    wait(|| view.status.text().contains("last good content retained"));
    click(&view, "Clear highlights");
    assert!(
        view.status.text().contains("last good content retained"),
        "clearing highlights must not hide a failed reload"
    );
    assert!(
        view.text
            .buffer()
            .text(
                &view.text.buffer().start_iter(),
                &view.text.buffer().end_iter(),
                true
            )
            .contains("changed_after_atomic")
    );
    std::fs::write(&path, &updated).unwrap();
    wait(|| !view.status.text().contains("last good content retained"));
    std::fs::remove_file(&path).unwrap();
    wait(|| {
        view.rendered
            .borrow()
            .as_ref()
            .is_some_and(|rendered| rendered.rows.iter().all(|row| row.coordinate.new.is_none()))
    });
    std::fs::write(&path, &changed).unwrap();
    wait(|| {
        view.rendered
            .borrow()
            .as_ref()
            .is_some_and(|rendered| rendered.rows.iter().any(|row| row.coordinate.new.is_some()))
    });
    window.set_child(gtk::Widget::NONE);
    drop(view);
    pump(100);
    gtk::Settings::default()
        .unwrap()
        .set_gtk_application_prefer_dark_theme(true);
    let view = SourceView::new(&document, "comparison-dark-validation", Rc::new(|_| {}));
    window.set_child(Some(&view.widget()));
    wait(|| view.rendered.borrow().is_some());
    capture(&window, "source-comparison-dark.png");
    let weak = Rc::downgrade(&view);
    window.destroy();
    drop(view);
    pump(100);
    assert!(weak.upgrade().is_none());
    gtk::Settings::default()
        .unwrap()
        .set_gtk_application_prefer_dark_theme(false);
    std::fs::remove_dir_all(&directory).unwrap();
}
