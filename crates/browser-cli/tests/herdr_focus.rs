//! Real sync process against private sockets; never touches the desktop services.
use serde_json::{Value, json};
use smad_protocol::{Command, PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope, ResponseStatus};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    process::{Child, Command as ProcessCommand, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type Focus = Option<(String, String)>;

fn read(stream: &mut UnixStream) -> Value {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}
fn send(stream: &mut UnixStream, value: Value) {
    writeln!(stream, "{value}").unwrap();
}
fn accept(listener: &UnixListener) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "sync process did not request a snapshot"
                );
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("{error}"),
        }
    }
}
fn subscribe(listener: &UnixListener) -> UnixStream {
    let mut stream = accept(listener);
    let request = read(&mut stream);
    assert_eq!(request["method"], "events.subscribe");
    send(&mut stream, json!({"id":request["id"], "result":{}}));
    stream
}
fn snapshot(listener: &UnixListener, focus: Option<(&str, &str)>) {
    let mut stream = accept(listener);
    let request = read(&mut stream);
    assert_eq!(request["method"], "session.snapshot");
    send(
        &mut stream,
        json!({"id":request["id"], "result":{"snapshot":{
            "focused_workspace_id":focus.map(|f|f.0), "focused_tab_id":focus.map(|f|f.1),
            "tabs":[{"workspace_id":"w1","tab_id":"t1","label":"One"},
                    {"workspace_id":"w2","tab_id":"t2","label":"Two"}]
        }}}),
    );
}
fn renames(rx: &mpsc::Receiver<Command>) {
    for _ in 0..2 {
        let command = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(
            matches!(command, Command::RenameHerdrProject { .. }),
            "unexpected command while waiting for labels: {command:?}"
        );
    }
}
// A subsequent snapshot request is a protocol barrier: the prior focus command,
// if any, has completed. No sleep-based assertion of "no event yet" is needed.
fn barrier(
    events: &mut UnixStream,
    listener: &UnixListener,
    rx: &mpsc::Receiver<Command>,
    focus: Option<(&str, &str)>,
) {
    send(events, json!({"event":"tab_renamed","data":{}}));
    snapshot(listener, focus);
    renames(rx);
}
struct Cleanup {
    child: Child,
    stop: Arc<AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
    directory: PathBuf,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn duplicate_focus_and_reconnect_preserve_manual_selection() {
    let directory = std::env::temp_dir().join(format!(
        "smad-focus-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let herdr_path = directory.join("herdr.sock");
    let browser_path = directory.join("browser.sock");
    let herdr = UnixListener::bind(&herdr_path).unwrap();
    herdr.set_nonblocking(true).unwrap();
    let browser = UnixListener::bind(&browser_path).unwrap();
    browser.set_nonblocking(true).unwrap();
    let selected: Arc<Mutex<Focus>> = Arc::new(Mutex::new(None));
    let observed = selected.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        while !stopped.load(Ordering::Relaxed) {
            let (mut stream, _) = match browser.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            let request: RequestEnvelope = serde_json::from_value(read(&mut stream)).unwrap();
            if let Command::SelectHerdrProject {
                workspace_id,
                tab_id,
                ..
            } = &request.command
            {
                *observed.lock().unwrap() = Some((workspace_id.clone(), tab_id.clone()));
            }
            tx.send(request.command).unwrap();
            let response = ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: request.request_id,
                status: ResponseStatus::Ok,
                error_code: None,
                message: None,
                state: None,
            };
            writeln!(stream, "{}", serde_json::to_string(&response).unwrap()).unwrap();
        }
    });
    let executable = std::env::var_os("SMAD_TEST_HERDR_SYNC_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_smad-herdr-sync").into());
    let child = ProcessCommand::new(executable)
        .args(["--session", "main", "--socket"])
        .arg(&herdr_path)
        .env("SMAD_SOCKET", &browser_path)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _cleanup = Cleanup {
        child,
        stop,
        server: Some(server),
        directory,
    };
    let mut events = subscribe(&herdr);
    snapshot(&herdr, Some(("w1", "t1")));
    renames(&rx);
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        Command::SelectHerdrProject { .. }
    ));
    let manual = Some(("manual-workspace".into(), "manual-tab".into()));
    *selected.lock().unwrap() = manual.clone();
    for event in ["tab_focused", "workspace_focused"] {
        send(&mut events, json!({"event":event,"data":{}}));
        snapshot(&herdr, Some(("w1", "t1")));
        renames(&rx);
        barrier(&mut events, &herdr, &rx, Some(("w1", "t1")));
        assert_eq!(
            *selected.lock().unwrap(),
            manual,
            "duplicate focus undid manual selection"
        );
    }
    drop(events);
    events = subscribe(&herdr);
    snapshot(&herdr, Some(("w1", "t1")));
    renames(&rx);
    barrier(&mut events, &herdr, &rx, Some(("w1", "t1")));
    assert_eq!(
        *selected.lock().unwrap(),
        manual,
        "reconnect undid manual selection"
    );

    for focus in [Some(("w2", "t2")), None, Some(("w1", "t1"))] {
        send(&mut events, json!({"event":"tab_focused","data":{}}));
        snapshot(&herdr, focus);
        renames(&rx);
        if let Some((workspace, tab)) = focus {
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(10)).unwrap(),
                Command::SelectHerdrProject {
                    session_name: "main".into(),
                    workspace_id: workspace.into(),
                    tab_id: tab.into()
                }
            );
            assert_eq!(
                *selected.lock().unwrap(),
                Some((workspace.into(), tab.into()))
            );
        }
        barrier(&mut events, &herdr, &rx, focus);
    }
    assert!(rx.try_recv().is_err());
}
