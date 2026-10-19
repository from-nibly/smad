//! Agent-facing commands over the existing single-instance document server.
#[path = "axi_compare.rs"]
mod compare;
use crate::{
    ProcessRunner, SystemRunner, ensure_browser, parse_lavish_toon, request, resolve_project,
    upstream_arguments,
};
use serde_json::{Value, json};
use smad_control::{BrowserControl, ControlError, SocketClient, default_socket_path};
use smad_core::document_uri::DocumentUri;
use smad_protocol::{
    BrowserStateSnapshot, Command, DocumentKind, DocumentLifecycleSnapshot, ResponseStatus,
};
use std::time::{Duration, Instant};

const DESCRIPTION: &str = "Show Me Agent Document: open explicitly typed local documents in a persistent native desktop reader.";
const OPEN_HELP: &str = "smad-axi open <code://file|md://file.md|lavish://file.html> [--reopen] [--timeout-ms 30000] [--json]";

struct Failure {
    code: &'static str,
    message: String,
    exit: i32,
}
impl Failure {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            code: "INVALID_ARGUMENT",
            message: message.into(),
            exit: 2,
        }
    }
    fn runtime(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            exit: 1,
        }
    }
}

type Result<T> = std::result::Result<T, Failure>;

pub fn run(mut args: Vec<String>) -> i32 {
    let json_output = args.iter().any(|arg| arg == "--json");
    args.retain(|arg| arg != "--json");
    match execute(&args) {
        Ok(value) => {
            output(&value, json_output);
            0
        }
        Err(error) => {
            output(
                &json!({"error": {"code": error.code, "message": error.message}, "help": ["smad-axi --help", OPEN_HELP]}),
                json_output,
            );
            error.exit
        }
    }
}

fn output(value: &Value, json_output: bool) {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(value).expect("JSON value")
        );
    } else {
        println!(
            "{}",
            toon_format::encode_default(value).expect("bounded output is TOON encodable")
        );
    }
}

fn execute(args: &[String]) -> Result<Value> {
    if matches!(args, [flag] if matches!(flag.as_str(), "--version" | "-V" | "-v")) {
        return Ok(json!({"name": "smad-axi", "version": env!("CARGO_PKG_VERSION")}));
    }
    if args.is_empty() {
        return dashboard(false);
    }
    if matches!(args, [flag] if flag == "--help" || flag == "help") {
        return Ok(help(None));
    }
    if let [command, flag] = args
        && flag == "--help"
    {
        return match command.as_str() {
            "open" | "compare" | "status" | "documents" | "schemes" | "setup" => {
                Ok(help(Some(command)))
            }
            _ => Err(Failure::usage(format!("unknown command: {command}"))),
        };
    }
    match args[0].as_str() {
        "status" | "documents" => {
            if args.len() > 2 || args.get(1).is_some_and(|flag| flag != "--full") {
                return Err(Failure::usage(
                    "status/documents accept only --full and --json",
                ));
            }
            dashboard(args.len() == 2)
        }
        "open" => open(&args[1..]),
        "compare" => compare::run(&args[1..]),
        "schemes" if args.len() == 1 => Ok(json!({
            "schemes": [
                {"scheme": "code://", "renderer": "native GTK source", "target": "whole local UTF-8 file; optional ?highlight=12-24,48 (1-based inclusive source lines)"},
                {"scheme": "md://", "renderer": "native GTK Markdown", "target": "local .md/.markdown file; optional ?highlight=12-24,48 and #heading"},
                {"scheme": "lavish://", "renderer": "upstream Lavish + WebKit", "target": "local .html/.htm artifact"}
            ],
            "paths": "After :// is a local path, never a network host. Relative paths use the invoking directory. Percent-encode literal spaces, # and ?.",
            "help": ["smad-axi open md://./README.md", "smad-axi open lavish://./review.html"]
        })),
        "setup" if args.get(1).is_some_and(|arg| arg == "hooks") && args.len() == 2 => Ok(json!({
            "command": "smad-axi status",
            "effect": "read-only session-start context; never starts or raises the server",
            "installed": false,
            "help": ["Configure your agent's session-start hook to run this command. No agent configuration was changed.", "An installable skill ships at share/smad/skills/smad/SKILL.md."]
        })),
        _ => Err(Failure::usage(format!(
            "unknown command or flags: {}",
            args.join(" ")
        ))),
    }
}

fn help(command: Option<&str>) -> Value {
    match command {
        Some("open") => {
            json!({"usage": OPEN_HELP, "behavior": "Capture invoking project; reuse/start SMAD; open via explicit renderer; verify the target reaches Ready. --reopen only reopens user-ended Lavish sessions when explicitly requested.", "coordinates": "code:// and md:// accept ?highlight=12-24,48 using original 1-based inclusive source lines; Markdown maps text precisely or whole diagram/image widgets and warns for unmapped syntax", "examples": ["smad-axi open 'code://./src/main.rs?highlight=12-24,48'", "smad-axi open md://./README.md", "smad-axi open 'md:///tmp/a%20file.md#heading'", "smad-axi open lavish://./review.html"], "help": ["smad-axi schemes"]})
        }
        Some("compare") => {
            json!({"usage": compare::HELP, "behavior": "Whole-file unified comparison, including all unchanged context. Exact --base by default; --merge-base requires --head and resolves a single merge base once. Working-tree after is live; the base stays fixed. --old/--new capture immutable files; /dev/null explicitly denotes an absent side. --highlight uses new-side source lines; --highlight-old uses old-side lines. No diff:// scheme and no patch application.", "examples": ["smad-axi compare ./src/main.rs --base HEAD --highlight 12-24,48", "smad-axi compare ./src/main.rs --base main --head feature --merge-base", "smad-axi compare --old ./before.rs --new ./after.rs"]})
        }
        Some("status" | "documents") => {
            json!({"usage": "smad-axi status|documents [--full] [--json]", "behavior": "Read-only. At most 20 document rows by default; --full includes every row without path truncation. Never starts or raises the server.", "help": [OPEN_HELP]})
        }
        Some("setup") => {
            json!({"usage": "smad-axi setup hooks", "behavior": "Print read-only session-start integration instructions; does not modify agent configuration."})
        }
        Some("schemes") => {
            json!({"usage": "smad-axi schemes [--json]", "behavior": "Describe the explicit local-file renderer schemes."})
        }
        _ => {
            json!({"usage": "smad-axi <command> [args] [flags]", "description": DESCRIPTION, "commands": [OPEN_HELP, compare::HELP, "status [--full] [--json]", "documents [--full] [--json]", "schemes [--json]", "setup hooks", "--version"], "no_arguments": "Show current server/document state without starting anything", "environment": {"SMAD_SOCKET": "Override the same-user Unix control socket", "SMAD_EXECUTABLE": "Override server executable; default is sibling smad, then PATH", "XDG_STATE_HOME": "Persistent state root (smad/state.json); legacy metadata is read only when new state is absent"}})
        }
    }
}

fn client() -> SocketClient {
    SocketClient::with_timeout(default_socket_path(), Duration::from_secs(5))
}

fn inspect(client: &SocketClient) -> Result<Option<BrowserStateSnapshot>> {
    match client.request(&request(Command::InspectState)) {
        Ok(response) => response
            .state
            .map(Some)
            .ok_or_else(|| Failure::runtime("INVALID_RESPONSE", "server returned no state")),
        Err(ControlError::Absent(_)) => Ok(None),
        Err(error) => Err(Failure::runtime("CONTROL_ERROR", error.to_string())),
    }
}

fn shorten(value: &str, full: bool) -> String {
    if full || value.chars().count() <= 120 {
        value.into()
    } else {
        format!(
            "{}… ({} chars; --full)",
            value.chars().take(96).collect::<String>(),
            value.chars().count()
        )
    }
}

fn dashboard(full: bool) -> Result<Value> {
    let bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "smad-axi".into());
    let Some(state) = inspect(&client())? else {
        return Ok(
            json!({"bin": bin, "description": DESCRIPTION, "server": "absent", "count": 0, "documents": [], "help": [OPEN_HELP, "smad-axi schemes"]}),
        );
    };
    let total: usize = state.projects.iter().map(|p| p.documents.len()).sum();
    let mut rows = Vec::new();
    let mut ready = 0;
    let mut failed = 0;
    for project in &state.projects {
        for document in &project.documents {
            ready += usize::from(document.lifecycle == DocumentLifecycleSnapshot::Ready);
            failed += usize::from(document.lifecycle == DocumentLifecycleSnapshot::Failed);
            if full || rows.len() < 20 {
                rows.push(json!({"project": shorten(&project.label, full), "renderer": renderer(document.kind), "file": shorten(&document.source_file, full), "state": if document.lifecycle == DocumentLifecycleSnapshot::Ready && document.diagnostic.is_some() { json!("ready_with_warnings") } else { json!(document.lifecycle) }}));
            }
        }
    }
    Ok(
        json!({"bin": bin, "description": DESCRIPTION, "server": {"state": "running", "pid": state.process_id, "version": state.version, "projects": state.projects.len(), "ready": ready, "failed": failed}, "count": total, "shown": rows.len(), "documents": rows, "help": [OPEN_HELP, "smad-axi documents --full", "Use md:// for native Markdown; lavish:// keeps upstream review behavior."]}),
    )
}

fn renderer(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Markdown => "markdown",
        DocumentKind::LavishReview => "lavish",
        DocumentKind::Code => "code",
        DocumentKind::Comparison => "comparison",
    }
}

fn open(args: &[String]) -> Result<Value> {
    let Some(uri) = args.first().filter(|arg| !arg.starts_with('-')) else {
        return Err(Failure::usage(OPEN_HELP));
    };
    let mut reopen = false;
    let mut timeout = Duration::from_secs(30);
    let mut timeout_seen = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--reopen" if !reopen => reopen = true,
            "--timeout-ms" if !timeout_seen => {
                timeout_seen = true;
                index += 1;
                let millis = args
                    .get(index)
                    .and_then(|s| s.parse::<u64>().ok())
                    .filter(|value| (100..=120_000).contains(value))
                    .ok_or_else(|| {
                        Failure::usage("--timeout-ms requires an integer from 100 through 120000")
                    })?;
                timeout = Duration::from_millis(millis);
            }
            flag => {
                return Err(Failure::usage(format!(
                    "unknown or duplicate open argument: {flag}"
                )));
            }
        }
        index += 1;
    }
    let cwd = std::env::current_dir().map_err(|e| Failure::runtime("FILESYSTEM", e.to_string()))?;
    let document = DocumentUri::parse(uri, &cwd).map_err(Failure::usage)?;
    if reopen && document.kind != DocumentKind::LavishReview {
        return Err(Failure::usage(
            "--reopen is only for lavish:// review sessions",
        ));
    }
    let project =
        resolve_project(&SystemRunner).map_err(|e| Failure::runtime("PROJECT_CONTEXT", e))?;
    let source_file = document.path.to_str().expect("validated UTF-8").to_owned();
    let command = match document.kind {
        DocumentKind::Code => Command::OpenCode {
            project: project.clone(),
            source_file: source_file.clone(),
            highlights: document.highlights.clone(),
        },
        DocumentKind::Comparison => return Err(Failure::usage("use smad-axi compare")),
        DocumentKind::Markdown => Command::OpenMarkdown {
            project: project.clone(),
            source_file: source_file.clone(),
            fragment: document.fragment.clone(),
            highlights: document.highlights.clone(),
        },
        DocumentKind::LavishReview => {
            let mut args = vec![source_file.clone()];
            if reopen {
                args.push("--reopen".into());
            }
            let result = SystemRunner
                .output("npx", &upstream_arguments(args))
                .map_err(|e| Failure::runtime("LAVISH_START_FAILED", e))?;
            if !result.status.success() {
                return Err(Failure::runtime(
                    "LAVISH_ERROR",
                    format!(
                        "{}: {}",
                        result.status,
                        shorten(&String::from_utf8_lossy(&result.stderr), false)
                    ),
                ));
            }
            let session = parse_lavish_toon(std::str::from_utf8(&result.stdout).map_err(|_| {
                Failure::runtime("LAVISH_RESPONSE", "upstream output was not UTF-8")
            })?)
            .map_err(|e| Failure::runtime("LAVISH_RESPONSE", e))?;
            if session.status == "user-ended" {
                return Err(Failure::runtime(
                    "SESSION_ENDED",
                    "User ended this review. Pass --reopen only with explicit permission to reopen it.",
                ));
            }
            if std::path::Path::new(&session.file)
                .canonicalize()
                .ok()
                .as_ref()
                != Some(&document.path)
            {
                return Err(Failure::runtime(
                    "SOURCE_MISMATCH",
                    "upstream returned a session for a different source file",
                ));
            }
            Command::OpenUrl {
                project: project.clone(),
                source_file: source_file.clone(),
                url: session.url,
            }
        }
    };
    open_command(
        command,
        project,
        source_file,
        document.kind,
        Some(document.canonical_uri()),
        timeout,
    )
}

fn open_command(
    command: Command,
    project: smad_protocol::ProjectMetadata,
    source_file: String,
    kind: DocumentKind,
    uri: Option<String>,
    timeout: Duration,
) -> Result<Value> {
    let control = client();
    ensure_browser(&control).map_err(|e| Failure::runtime("SERVER_START_FAILED", e))?;
    let server = inspect(&control)?
        .ok_or_else(|| Failure::runtime("SERVER_ABSENT", "server disappeared during startup"))?;
    if !server
        .runtime_id
        .as_deref()
        .is_some_and(|id| id.starts_with("works.from-nibly.Smad:"))
    {
        return Err(Failure::runtime(
            "INCOMPATIBLE_SERVER",
            "socket belongs to a legacy/different application; run the matching SMAD server",
        ));
    }
    if matches!(kind, DocumentKind::Code | DocumentKind::Comparison)
        && !server
            .capabilities
            .iter()
            .any(|capability| capability == "source_documents_v1")
    {
        return Err(Failure::runtime(
            "INCOMPATIBLE_SERVER",
            "running SMAD does not support native source documents; upgrade/restart it before opening code or comparisons",
        ));
    }
    let markdown_ranges_supported = server
        .capabilities
        .iter()
        .any(|c| c == "markdown_source_ranges_v1");
    if matches!(&command, Command::OpenMarkdown { highlights, .. } if !highlights.is_empty())
        && !markdown_ranges_supported
    {
        return Err(Failure::runtime(
            "INCOMPATIBLE_SERVER",
            "running SMAD does not support Markdown source highlights; upgrade/restart it first",
        ));
    }
    let envelope = request(command);
    let response = control
        .request(&envelope)
        .map_err(|e| Failure::runtime("OPEN_REJECTED", e.to_string()))?;
    if response.status != ResponseStatus::Ok {
        return Err(Failure::runtime(
            "OPEN_REJECTED",
            response.message.unwrap_or_default(),
        ));
    }
    let deadline = Instant::now() + timeout;
    loop {
        let state = inspect(&control)?.ok_or_else(|| {
            Failure::runtime("SERVER_ABSENT", "server exited while loading the document")
        })?;
        let target = state
            .projects
            .iter()
            .find(|p| p.key == project.key)
            .and_then(|p| {
                p.documents
                    .iter()
                    .find(|d| d.source_file == source_file && d.kind == kind)
            });
        if let Some(target) = target {
            if (matches!(kind, DocumentKind::Code | DocumentKind::Comparison)
                || (kind == DocumentKind::Markdown && markdown_ranges_supported))
                && target.open_request_id.as_deref() != Some(&envelope.request_id)
            {
                return Err(Failure::runtime(
                    "OPEN_SUPERSEDED",
                    "another action replaced or cleared this document request; inspect current state rather than assuming these ranges/inputs are still displayed",
                ));
            }
            match target.lifecycle {
                DocumentLifecycleSnapshot::Ready => return Ok(json!({"document": {"uri": uri, "file": source_file, "renderer": renderer(kind), "state": if target.diagnostic.is_some() { "ready_with_warnings" } else { "ready" }, "project": project.label}, "warnings": target.diagnostic.iter().collect::<Vec<_>>(), "server_pid": state.process_id, "help": ["smad-axi documents", if kind == DocumentKind::LavishReview { "Use lavish-axi poll/reply/export for the upstream review workflow." } else { "Native read-only document. Source ranges use original 1-based lines, not wrapped/display rows." }]})),
                DocumentLifecycleSnapshot::Failed => return Err(Failure::runtime("DOCUMENT_FAILED", target.diagnostic.clone().unwrap_or_else(|| "Document opened but failed to load; inspect its visible diagnostic and the SMAD server log".into()))),
                _ => {}
            }
        }
        if Instant::now() >= deadline {
            return Err(Failure::runtime(
                "READINESS_TIMEOUT",
                "Document was opened but did not reach Ready before the deadline; use smad-axi status. Do not assume it failed or retry blindly.",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_commands_and_flags_fail_before_any_side_effect() {
        for args in [
            vec!["unknown"],
            vec!["open", "md://x.md", "--invented"],
            vec!["status", "--limit", "3"],
            vec!["open", "md://x.md", "--timeout-ms", "-1"],
        ] {
            let args: Vec<_> = args.into_iter().map(str::to_owned).collect();
            assert!(matches!(execute(&args), Err(Failure { exit: 2, .. })));
        }
    }
    #[test]
    fn help_and_schemes_are_bounded_machine_readable_data() {
        let value = execute(&["schemes".into()]).unwrap_or_else(|_| panic!("schemes"));
        assert_eq!(value["schemes"].as_array().unwrap().len(), 3);
        let encoded = toon_format::encode_default(&value).unwrap();
        assert_eq!(
            toon_format::decode_default::<Value>(&encoded).unwrap(),
            value
        );
        assert!(
            help(Some("open"))["usage"]
                .as_str()
                .unwrap()
                .contains("md://")
        );
    }
}
