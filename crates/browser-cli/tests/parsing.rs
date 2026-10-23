use smad_cli::{
    LavishSession, parse_lavish_toon, project_from_herdr_tabs, project_from_panes,
    upstream_arguments,
};
use smad_protocol::ProjectKey;

#[test]
fn parses_current_toon_and_preserves_loopback_output() {
    let text = "session:\n  file: \"/tmp/review file.html\"\n  url: \"http://[::1]:4387/session/key?noGate=1\"\n  status: opened\nnext_step: ignored\n";
    assert_eq!(
        parse_lavish_toon(text).unwrap(),
        LavishSession {
            file: "/tmp/review file.html".into(),
            url: "http://[::1]:4387/session/key?noGate=1".into(),
            status: "opened".into(),
        }
    );
    assert!(parse_lavish_toon("session:\n  file: /tmp/x\n").is_err());
}

#[test]
fn advertised_upstream_hosts_are_localized_without_dns_or_remote_access() {
    for advertised in [
        "http://workstation.example.ts.net:4387/session/key?noGate=1#review",
        "http://192.0.2.12:4387/session/key?noGate=1#review",
        "http://[2001:db8::1]:4387/session/key?noGate=1#review",
        "http://must-not-resolve.invalid:4387/session/key?noGate=1#review",
    ] {
        let parsed = parse_lavish_toon(&format!(
            "session:\n  file: /tmp/x\n  url: {advertised}\n  status: opened\n"
        ))
        .unwrap();
        assert_eq!(
            parsed.url,
            "http://127.0.0.1:4387/session/key?noGate=1#review"
        );
        assert!(
            smad_protocol::validate_session_url(advertised).is_err(),
            "raw remote URLs must remain rejected"
        );
        assert!(smad_protocol::validate_session_url(&parsed.url).is_ok());
    }
}

#[test]
fn localizing_upstream_hosts_does_not_accept_unsafe_urls() {
    for url in [
        "https://user:secret@workstation.example.ts.net:4387/session/key",
        "ftp://workstation.example.ts.net:4387/session/key",
        "javascript:alert(1)",
        "file:///session/key",
        "http://workstation.example.ts.net:4387/not-session/key",
        "http://workstation.example.ts.net:4387/session/",
        "http://workstation.example.ts.net:4387/session//key",
        "http://workstation.example.ts.net:bad/session/key",
    ] {
        assert!(
            parse_lavish_toon(&format!(
                "session:\n  file: /tmp/x\n  url: {url}\n  status: opened\n"
            ))
            .is_err(),
            "{url}"
        );
    }
}

#[test]
fn adds_no_open_exactly_once_without_reordering_flags() {
    assert_eq!(
        upstream_arguments(vec!["file.html".into(), "--no-gate".into()]),
        ["-y", "lavish-axi", "file.html", "--no-gate", "--no-open"].map(str::to_owned)
    );
    assert_eq!(
        upstream_arguments(vec![
            "file.html".into(),
            "--no-open".into(),
            "--reopen".into()
        ]),
        ["-y", "lavish-axi", "file.html", "--no-open", "--reopen"].map(str::to_owned)
    );
}

#[test]
fn maps_the_invoking_pane_instead_of_the_active_tab() {
    let panes = br#"[
      {"id": 4, "tab_id": 90, "tab_name": "active but wrong", "is_focused": true},
      {"id": 17, "tab_id": 6, "tab_name": "__super_tabs_id=\"x\" | directory=\"api\" | worktree=\"feature\"", "is_focused": false}
    ]"#;
    let project = project_from_panes("main".into(), 17, panes).unwrap();
    assert_eq!(
        project.key,
        ProjectKey::Zellij {
            session_name: "main".into(),
            stable_tab_id: 6
        }
    );
    assert_eq!(project.label, "api · feature");
}

#[test]
fn maps_the_invoking_herdr_tab_by_stable_string_identity() {
    let tabs = br#"{
      "id":"cli:tab:list",
      "result":{"tabs":[
        {"workspace_id":"w2","tab_id":"w2:t1","label":"wrong"},
        {"workspace_id":"w3","tab_id":"w3:tA","label":"home-manager"}
      ]}
    }"#;
    let project =
        project_from_herdr_tabs("main".into(), "w3".into(), "w3:tA".into(), tabs).unwrap();
    assert_eq!(
        project.key,
        ProjectKey::Herdr {
            session_name: "main".into(),
            workspace_id: "w3".into(),
            tab_id: "w3:tA".into(),
        }
    );
    assert_eq!(project.label, "home-manager");
}
