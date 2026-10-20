use super::*;

#[test]
#[ignore = "requires isolated GTK display; one GTK test per process"]
fn project_click_is_committed_before_deferred_render() {
    gtk::init().unwrap();
    let notebook = gtk::Notebook::new();
    let window = gtk::ApplicationWindow::builder().child(&notebook).build();
    let a = ProjectKey::Standalone { label: "A".into() };
    let b = ProjectKey::Standalone { label: "B".into() };
    let mut model = BrowserModel::default();
    for key in [&a, &b] {
        model.projects.push(smad_core::Project {
            key: key.clone(),
            label: format!("{key:?}"),
            raw_tab_name: None,
            documents: Vec::new(),
            selected_document: None,
            selected_kind: DocumentKind::LavishReview,
            created_at: 1,
            updated_at: 1,
        });
    }
    model.selected_project = Some(a.clone());
    let directory = std::env::temp_dir().join(format!("smad-selection-{}", std::process::id()));
    let controller = Rc::new(AppController {
        window,
        notebook,
        download_status: gtk::Label::new(None),
        downloads_configured: Cell::new(false),
        model: RefCell::new(model),
        store: MetadataStore::new(&directory),
        rendering: Cell::new(false),
        presentation_count: Cell::new(0),
        document_views: RefCell::new(webview::RetainedRegistry::default()),
        project_stacks: RefCell::new(HashMap::new()),
        memory_monitor: gio::MemoryMonitor::dup_default(),
    });
    controller.connect_notebook_selection();
    controller.render();
    let context = glib::MainContext::default();
    while context.pending() {
        context.iteration(false);
    }

    // switch-page is the same signal GTK emits on a tab click. Do not let the
    // deferred idle callback run before a background render (e.g. a load event).
    controller.notebook.set_current_page(Some(1));
    assert_eq!(
        controller.model.borrow().selected_project,
        Some(b.clone()),
        "tab click must update the model before another event can render"
    );
    controller.render();
    assert_eq!(
        controller.notebook.current_page(),
        Some(1),
        "background render snapped back"
    );
    while context.pending() {
        context.iteration(false);
    }
    assert_eq!(controller.notebook.current_page(), Some(1));

    // The deferred work must render the latest selection, not replay an old key.
    controller.notebook.set_current_page(Some(0));
    controller.mutate_with_render(|model| model.select_project(&b, 10), false);
    while context.pending() {
        context.iteration(false);
    }
    assert_eq!(controller.model.borrow().selected_project, Some(b));
    assert_eq!(controller.notebook.current_page(), Some(1));
    controller.window.destroy();
    drop(controller);
    std::fs::remove_dir_all(directory).unwrap();
}
