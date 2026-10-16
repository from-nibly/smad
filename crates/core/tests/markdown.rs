use smad_core::persistence::MetadataStore;
use smad_core::{BrowserModel, BrowserStateStore, DocumentLifecycle, ProjectKey};
use smad_protocol::{DocumentKind, ProjectMetadata};

fn project(tab: &str) -> ProjectMetadata {
    ProjectMetadata {
        key: ProjectKey::Herdr {
            session_name: "main".into(),
            workspace_id: "workspace".into(),
            tab_id: tab.into(),
        },
        label: "same display label".into(),
        raw_tab_name: None,
    }
}

#[test]
fn native_documents_deduplicate_within_stable_project_and_survive_restore() {
    let directory = std::env::temp_dir().join(format!("lavish-md-model-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut model = BrowserModel::default();
    assert!(
        model
            .open_markdown(project("tab-1"), "/tmp/guide.md".into(), 1)
            .document_created
    );
    assert!(
        !model
            .open_markdown(project("tab-1"), "/tmp/guide.md".into(), 2)
            .document_created
    );
    assert!(
        model
            .open_markdown(project("tab-2"), "/tmp/guide.md".into(), 3)
            .document_created
    );
    model.open_url(
        project("tab-1"),
        "/tmp/review.html",
        "http://127.0.0.1:4387/session/test",
        4,
    );
    let store = MetadataStore::new(&directory);
    store.save(&model).unwrap();
    let restored = store.load().unwrap();
    assert_eq!(restored.projects.len(), 2);
    let documents = &restored.projects[0].documents;
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].kind, DocumentKind::Markdown);
    assert_eq!(documents[1].kind, DocumentKind::LavishReview);
    assert_eq!(documents[0].lifecycle, DocumentLifecycle::Dormant);
    assert_eq!(documents[0].lavish_url, "");
    assert_eq!(
        restored.projects[1].documents[0].kind,
        DocumentKind::Markdown
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn read_limits_preserve_invalid_utf8_and_reject_oversized_input() {
    let path = std::env::temp_dir().join(format!("lavish-md-read-{}.md", std::process::id()));
    std::fs::write(&path, [0xff, 0xfe]).unwrap();
    assert!(smad_core::markdown::read_markdown(&path).is_err());
    std::fs::write(
        &path,
        vec![b'x'; smad_core::markdown::MAX_MARKDOWN_BYTES as usize + 1],
    )
    .unwrap();
    assert!(smad_core::markdown::read_markdown(&path).is_err());
    std::fs::remove_file(&path).unwrap();
    let fifo = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    // SAFETY: fifo is a live, NUL-terminated path owned by this test.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(smad_core::markdown::read_markdown(&path).is_err());
    std::fs::remove_file(path).unwrap();
}
