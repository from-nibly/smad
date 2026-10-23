use smad_control::SocketServer;
use smad_protocol::{
    BrowserStateSnapshot, Command, DocumentKind, DocumentLifecycleSnapshot, DocumentSnapshot,
    PROTOCOL_VERSION, ProjectSnapshot, ResponseEnvelope, ResponseStatus,
};
use std::fs;
use std::process::Command as Process;
use std::sync::{Arc, Mutex};

fn directory(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "smad-axi-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}
fn cli(socket: &std::path::Path) -> Process {
    let mut command = Process::new(env!("CARGO_BIN_EXE_smad-axi"));
    command.env("SMAD_SOCKET", socket);
    for (name, _) in std::env::vars()
        .filter(|(name, _)| name.starts_with("HERDR_") || name.starts_with("ZELLIJ_"))
    {
        command.env_remove(name);
    }
    command
}

#[test]
fn absent_dashboard_and_unknown_flags_never_start_a_server() {
    let dir = directory("absent");
    let socket = dir.join("absent.sock");
    let result = cli(&socket)
        .env("SMAD_EXECUTABLE", "/must-not-execute")
        .arg("--json")
        .output()
        .unwrap();
    assert!(result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["server"], "absent");
    assert_eq!(value["count"], 0);
    let result = cli(&socket)
        .args(["open", "md://missing.md", "--invented", "--json"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["error"]["code"], "INVALID_ARGUMENT");
    assert!(!socket.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn lavish_scheme_localizes_advertised_host_and_preserves_upstream_no_open() {
    use std::os::unix::fs::PermissionsExt;
    let dir = directory("lavish");
    let file = dir.join("review.html");
    fs::write(&file, "<h1>Review</h1>").unwrap();
    let toon = format!(
        "session:\n  file: {}\n  url: http://workstation.example.ts.net:4387/session/axi-test\n  status: opened",
        file.display()
    );
    let stub = dir.join("npx");
    fs::write(&stub, format!("#!/usr/bin/env python3\nimport json, os, sys\njson.dump(sys.argv[1:], open(os.environ['ARGS_LOG'], 'w'))\nprint({})\n", serde_json::to_string(&toon).unwrap())).unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = dir.join("control.sock");
    let current = Arc::new(Mutex::new(None::<ProjectSnapshot>));
    let captured = current.clone();
    let server = SocketServer::bind(&socket, move |request| {
        if let Command::OpenUrl {
            project,
            source_file,
            url,
        } = &request.command
        {
            assert_eq!(url, "http://127.0.0.1:4387/session/axi-test");
            *captured.lock().unwrap() = Some(ProjectSnapshot {
                selected_kind: DocumentKind::LavishReview,
                key: project.key.clone(),
                label: project.label.clone(),
                selected_source_file: Some(source_file.clone()),
                documents: vec![DocumentSnapshot {
                    kind: DocumentKind::LavishReview,
                    open_request_id: None,
                    source_file: source_file.clone(),
                    url: url.clone(),
                    title: Some("Review".into()),
                    lifecycle: DocumentLifecycleSnapshot::Ready,
                    diagnostic: None,
                }],
            });
        }
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: request.request_id,
            status: ResponseStatus::Ok,
            error_code: None,
            message: None,
            state: matches!(request.command, Command::InspectState).then(|| BrowserStateSnapshot {
                capabilities: Vec::new(),
                version: Some("0.3.0".into()),
                projects: captured.lock().unwrap().iter().cloned().collect(),
                selected_project: None,
                presentation_count: 1,
                process_id: 42,
                runtime_id: Some("works.from-nibly.Smad:42".into()),
            }),
        }
    })
    .unwrap();
    let serving = std::thread::spawn(move || {
        for _ in 0..4 {
            server.accept().unwrap().join().unwrap();
        }
    });
    let log = dir.join("upstream-args.json");
    let result = cli(&socket)
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("ARGS_LOG", &log)
        .args(["open", &format!("lavish://{}", file.display()), "--json"])
        .output()
        .unwrap();
    serving.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["document"]["renderer"], "lavish");
    let args: Vec<String> = serde_json::from_slice(&fs::read(log).unwrap()).unwrap();
    assert_eq!(
        args,
        vec![
            "-y".to_owned(),
            "lavish-axi".into(),
            file.to_str().unwrap().into(),
            "--no-open".into()
        ]
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn open_returns_verified_target_state_and_reports_failures() {
    for failed in [false, true] {
        let dir = directory(if failed { "failed" } else { "ready" });
        let file = dir.join("a file.md");
        fs::write(&file, "# Reader").unwrap();
        let socket = dir.join("control.sock");
        let state = Arc::new(Mutex::new(BrowserStateSnapshot {
            capabilities: Vec::new(),
            version: Some("0.3.0".into()),
            projects: vec![],
            selected_project: None,
            presentation_count: 0,
            process_id: 42,
            runtime_id: Some("works.from-nibly.Smad:42".into()),
        }));
        let capture = state.clone();
        let server = SocketServer::bind(&socket, move |request| {
            let mut state = capture.lock().unwrap();
            if let Command::OpenMarkdown {
                project,
                source_file,
                fragment,
                ..
            } = &request.command
            {
                assert_eq!(fragment.as_deref(), Some("reader"));
                state.projects.push(ProjectSnapshot {
                    selected_kind: DocumentKind::Markdown,
                    key: project.key.clone(),
                    label: project.label.clone(),
                    selected_source_file: Some(source_file.clone()),
                    documents: vec![DocumentSnapshot {
                        kind: DocumentKind::Markdown,
                        open_request_id: None,
                        source_file: source_file.clone(),
                        url: String::new(),
                        title: Some("Reader".into()),
                        lifecycle: if failed {
                            DocumentLifecycleSnapshot::Failed
                        } else {
                            DocumentLifecycleSnapshot::Ready
                        },
                        diagnostic: failed.then(|| "simulated invalid document".into()),
                    }],
                });
            }
            ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: request.request_id,
                status: ResponseStatus::Ok,
                error_code: None,
                message: None,
                state: matches!(request.command, Command::InspectState).then(|| state.clone()),
            }
        })
        .unwrap();
        let serving = std::thread::spawn(move || {
            for _ in 0..4 {
                server.accept().unwrap().join().unwrap();
            }
        });
        let uri = format!("md://{}#reader", file.display());
        let result = cli(&socket)
            .args(["open", &uri, "--json"])
            .output()
            .unwrap();
        serving.join().unwrap();
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        if failed {
            assert_eq!(result.status.code(), Some(1));
            assert_eq!(value["error"]["code"], "DOCUMENT_FAILED");
            assert!(
                value["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("simulated")
            );
        } else {
            assert!(result.status.success(), "{value}");
            assert_eq!(value["document"]["state"], "ready");
            assert_eq!(value["document"]["renderer"], "markdown");
            assert!(
                value["document"]["uri"]
                    .as_str()
                    .unwrap()
                    .contains("a%20file.md#reader")
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn source_opens_require_capability_and_their_own_request_identity() {
    for (kind, scheme, capability, capable, superseded) in [
        (DocumentKind::Code, "code", "source_documents_v1"),
        (DocumentKind::Markdown, "md", "markdown_source_ranges_v1"),
    ]
    .into_iter()
    .flat_map(|(kind, scheme, capability)| {
        [(false, false), (true, true), (true, false)]
            .map(|(capable, superseded)| (kind, scheme, capability, capable, superseded))
    }) {
        let dir = directory("source");
        let file = dir.join("source.md");
        fs::write(&file, "fn main() {}\n").unwrap();
        let socket = dir.join("control.sock");
        let state = Arc::new(Mutex::new(BrowserStateSnapshot {
            capabilities: if capable {
                vec![capability.into()]
            } else {
                Vec::new()
            },
            version: Some("0.4.0".into()),
            projects: Vec::new(),
            selected_project: None,
            presentation_count: 0,
            process_id: 42,
            runtime_id: Some("works.from-nibly.Smad:42".into()),
        }));
        let captured = state.clone();
        let server = SocketServer::bind(&socket, move |request| {
            let mut state = captured.lock().unwrap();
            if let Command::OpenCode {
                project,
                source_file,
                highlights,
            }
            | Command::OpenMarkdown {
                project,
                source_file,
                highlights,
                ..
            } = &request.command
            {
                assert!(capable);
                assert_eq!(smad_protocol::format_ranges(highlights), "2-4,48");
                state.projects = vec![ProjectSnapshot {
                    key: project.key.clone(),
                    label: project.label.clone(),
                    selected_kind: kind,
                    selected_source_file: Some(source_file.clone()),
                    documents: vec![DocumentSnapshot {
                        kind,
                        source_file: source_file.clone(),
                        url: String::new(),
                        title: None,
                        lifecycle: DocumentLifecycleSnapshot::Ready,
                        diagnostic: None,
                        open_request_id: Some(if superseded {
                            "another-open".into()
                        } else {
                            request.request_id.clone()
                        }),
                    }],
                }];
            }
            ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: request.request_id,
                status: ResponseStatus::Ok,
                error_code: None,
                message: None,
                state: matches!(request.command, Command::InspectState).then(|| state.clone()),
            }
        })
        .unwrap();
        let serving = std::thread::spawn(move || {
            for _ in 0..if capable { 4 } else { 2 } {
                server.accept().unwrap().join().unwrap();
            }
        });
        let result = cli(&socket)
            .env("SMAD_EXECUTABLE", "/must-not-execute")
            .args([
                "open",
                &format!("{scheme}://{}?highlight=48,2-3,4", file.display()),
                "--json",
            ])
            .output()
            .unwrap();
        serving.join().unwrap();
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        if !capable {
            assert_eq!(value["error"]["code"], "INCOMPATIBLE_SERVER");
        } else if superseded {
            assert_eq!(value["error"]["code"], "OPEN_SUPERSEDED");
        } else {
            assert!(result.status.success(), "{value}");
            assert_eq!(
                value["document"]["renderer"],
                if kind == DocumentKind::Markdown {
                    "markdown"
                } else {
                    "code"
                }
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
