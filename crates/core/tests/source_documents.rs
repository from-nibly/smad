use smad_core::{
    BrowserModel, comparison,
    document_uri::DocumentUri,
    git_source::GitSource,
    snapshots::SnapshotStore,
    source::{self, SourceText},
};
use smad_protocol::{DocumentKind, LineRange, ProjectKey, ProjectMetadata};
use std::{path::Path, process::Command};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn commit(root: &Path) {
    git(root, &["add", "--all"]);
    git(
        root,
        &[
            "-c",
            "user.name=SMAD test",
            "-c",
            "user.email=smad-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "fixture",
        ],
    );
}
#[test]
fn code_uri_queries_preserve_encoded_filenames_and_normalize_ranges() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("source?#%.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    let uri = DocumentUri::parse(
        "code://./source%3F%23%25.rs?highlight=8,2-4,4-5",
        directory.path(),
    )
    .unwrap();
    assert_eq!(uri.kind, DocumentKind::Code);
    assert_eq!(uri.path, file);
    assert_eq!(
        uri.highlights,
        vec![
            LineRange { start: 2, end: 5 },
            LineRange { start: 8, end: 8 }
        ]
    );
    assert_eq!(
        DocumentUri::parse(&uri.canonical_uri(), Path::new("/")).unwrap(),
        uri
    );
    for suffix in [
        "?highlight=0",
        "?highlight=3-1",
        "?highlight=1&highlight=2",
        "?unknown=1",
        "#heading",
        "?highlight=%ff",
        "?highlight=",
    ] {
        assert!(
            DocumentUri::parse(
                &format!("code://source%3F%23%25.rs{suffix}"),
                directory.path()
            )
            .is_err()
        );
    }
    assert!(DocumentUri::parse("diff://anything", directory.path()).is_err());
    assert!(source::canonical_location(Path::new("/tmp/missing\0.rs")).is_err());
    assert!(
        smad_core::snapshots::ComparisonSpec {
            before: None,
            after: None,
            live_after: None,
            before_label: "bad\0label".into(),
            after_label: "after".into(),
        }
        .validate()
        .is_err()
    );
}
#[test]
fn renderers_coexist_and_selection_and_closure_use_full_identity() {
    let project = ProjectMetadata {
        key: ProjectKey::Standalone {
            label: "source".into(),
        },
        label: "source".into(),
        raw_tab_name: None,
    };
    let mut model = BrowserModel::default();
    model.open_markdown(project.clone(), "/tmp/readme.md".into(), 1);
    let markdown = model.projects[0].documents[0].key.clone();
    model.open_document(
        project.clone(),
        "/tmp/readme.md".into(),
        String::new(),
        DocumentKind::Code,
        2,
    );
    let code = model.projects[0].documents[1].key.clone();
    assert_ne!(code, markdown);
    assert_eq!(model.projects[0].selected().unwrap().key, code);
    assert_eq!(model.inactive_documents_lru()[0].key, markdown);
    model.open_document(
        project,
        "/tmp/readme.md".into(),
        String::new(),
        DocumentKind::Code,
        3,
    );
    assert_eq!(model.projects[0].documents.len(), 2);
    assert!(model.select_document(&markdown, 4));
    assert!(model.close_document(&code));
    assert_eq!(model.projects[0].selected().unwrap().key, markdown);
}
#[test]
fn persistence_infers_legacy_markdown_selection_and_preserves_renderer_identity() {
    use smad_core::BrowserStateStore;
    let directory = tempfile::tempdir().unwrap();
    let store = smad_core::persistence::MetadataStore::new(directory.path());
    let project = ProjectMetadata {
        key: ProjectKey::Standalone {
            label: "selection".into(),
        },
        label: "selection".into(),
        raw_tab_name: None,
    };
    let mut model = BrowserModel::default();
    model.open_markdown(project.clone(), "/tmp/same.md".into(), 1);
    store.save(&model).unwrap();
    let mut legacy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(store.state_file()).unwrap()).unwrap();
    legacy["projects"][0]
        .as_object_mut()
        .unwrap()
        .remove("selected_kind");
    std::fs::write(store.state_file(), serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(
        store.load().unwrap().projects[0].selected().unwrap().kind,
        DocumentKind::Markdown
    );
    model.open_document(
        project,
        "/tmp/same.md".into(),
        String::new(),
        DocumentKind::Code,
        2,
    );
    model.projects[0].documents[1].highlights = vec![LineRange { start: 2, end: 4 }];
    store.save(&model).unwrap();
    let restored = store.load().unwrap();
    assert_eq!(restored.projects[0].documents.len(), 2);
    assert_eq!(
        restored.projects[0].selected().unwrap().kind,
        DocumentKind::Code
    );
    assert_eq!(
        restored.projects[0].selected().unwrap().highlights,
        vec![LineRange { start: 2, end: 4 }]
    );
}

#[test]
fn immutable_snapshots_deduplicate_and_detect_corruption() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(directory.path().join("snapshots"));
    let path = directory.path().join("source.rs");
    std::fs::write(&path, "before\r\n🦀").unwrap();
    let snapshot = store.capture(&path).unwrap();
    std::fs::write(&path, "after").unwrap();
    assert_eq!(store.read(&snapshot).unwrap().text, "before\r\n🦀");
    assert_eq!(
        store
            .put(&SourceText::parse("before\r\n🦀".into()).unwrap())
            .unwrap(),
        snapshot
    );
    assert_eq!(
        std::fs::read_dir(directory.path().join("snapshots"))
            .unwrap()
            .count(),
        1
    );
    std::fs::write(
        directory.path().join("snapshots").join(&snapshot.digest),
        "tampered",
    )
    .unwrap();
    assert!(store.read(&snapshot).is_err());
}
#[test]
fn exact_git_snapshots_remain_fixed_and_distinguish_absent_binary_and_large_blobs() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "--initial-branch=main"]);
    let path = root.join("source[?]#.rs");
    std::fs::write(&path, "before\n").unwrap();
    commit(root);
    let repository = GitSource::discover(&path).unwrap();
    let base = repository.resolve("HEAD").unwrap();
    std::fs::write(&path, "after\n").unwrap();
    std::fs::write(root.join("added.rs"), "added\n").unwrap();
    std::fs::write(root.join("binary.rs"), [0, 255]).unwrap();
    std::fs::write(
        root.join("large.rs"),
        vec![b'x'; source::MAX_SOURCE_BYTES + 1],
    )
    .unwrap();
    commit(root);
    let head = repository.resolve("HEAD").unwrap();
    assert_ne!(base, head);
    std::fs::write(&path, "working tree\n").unwrap();
    assert_eq!(repository.read(&base).unwrap().unwrap().text, "before\n");
    assert_eq!(repository.read(&head).unwrap().unwrap().text, "after\n");
    assert_eq!(source::read_source(&path).unwrap().text, "working tree\n");
    assert_eq!(repository.merge_base(&base, &head).unwrap(), base);
    assert!(repository.resolve("--help").is_err());
    assert!(repository.resolve("HEAD; touch injected").is_err());
    assert!(!root.join("injected").exists());
    assert!(
        GitSource::discover(&root.join("added.rs"))
            .unwrap()
            .read(&base)
            .unwrap()
            .is_none()
    );
    assert!(
        GitSource::discover(&root.join("binary.rs"))
            .unwrap()
            .read(&head)
            .is_err()
    );
    assert!(
        GitSource::discover(&root.join("large.rs"))
            .unwrap()
            .read(&head)
            .unwrap_err()
            .contains("1 MiB")
    );
    std::fs::remove_file(&path).unwrap();
    let deleted = GitSource::discover(&path).unwrap();
    assert_eq!(deleted.read(&base).unwrap().unwrap().text, "before\n");
    assert_eq!(
        comparison::compare(
            &deleted.read(&base).unwrap().unwrap(),
            &SourceText::parse(String::new()).unwrap()
        )
        .unwrap()
        .rows
        .len(),
        1
    );
}
