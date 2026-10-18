use super::{Failure, Result, SystemRunner, open_command, resolve_project};
use serde_json::{Value, json};
use smad_core::{
    git_source::GitSource,
    source::{self, SourceText},
};
use smad_protocol::{Command, DocumentKind, parse_ranges};
use std::{collections::HashMap, io::Write, path::Path, time::Duration};

pub(super) const HELP: &str = "smad-axi compare FILE --base REV [--head REV] [--merge-base] | compare --old FILE --new FILE [--highlight 12-24,48] [--highlight-old 3-8] [--timeout-ms 30000] [--json]";

fn snapshot(
    source: Option<SourceText>,
    temporaries: &mut Vec<tempfile::NamedTempFile>,
) -> Result<Option<String>> {
    let Some(source) = source else {
        return Ok(None);
    };
    let mut file = tempfile::NamedTempFile::new()
        .map_err(|error| Failure::runtime("SNAPSHOT", error.to_string()))?;
    file.write_all(source.text.as_bytes())
        .map_err(|error| Failure::runtime("SNAPSHOT", error.to_string()))?;
    let path = file
        .path()
        .to_str()
        .ok_or_else(|| Failure::runtime("SNAPSHOT", "temporary directory path is not UTF-8"))?
        .to_owned();
    temporaries.push(file);
    Ok(Some(path))
}
fn explicit_file(path: &str) -> Result<Option<SourceText>> {
    if path == "/dev/null" {
        return Ok(None);
    }
    source::read_source(Path::new(path))
        .map(Some)
        .map_err(|error| Failure::runtime("SOURCE", error))
}

pub(super) fn run(args: &[String]) -> Result<Value> {
    let mut flags = HashMap::new();
    let mut file = None;
    let mut merge_base = false;
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        if argument == "--merge-base" && !merge_base {
            merge_base = true;
        } else if matches!(
            argument.as_str(),
            "--old"
                | "--new"
                | "--base"
                | "--head"
                | "--highlight"
                | "--highlight-old"
                | "--timeout-ms"
        ) {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| Failure::usage(format!("{argument} requires a value")))?;
            if flags.insert(argument.as_str(), value.as_str()).is_some() {
                return Err(Failure::usage(format!("duplicate {argument}")));
            }
        } else if !argument.starts_with('-') && file.is_none() {
            file = Some(argument.as_str());
        } else {
            return Err(Failure::usage(format!(
                "unknown or duplicate comparison argument: {argument}"
            )));
        }
        index += 1;
    }
    let ranges = |flag| {
        flags
            .get(flag)
            .map(|value| parse_ranges(value))
            .transpose()
            .map(|value| value.unwrap_or_default())
            .map_err(Failure::usage)
    };
    let highlights = ranges("--highlight")?;
    let old_highlights = ranges("--highlight-old")?;
    let timeout = if let Some(value) = flags.get("--timeout-ms") {
        Duration::from_millis(
            value
                .parse::<u64>()
                .ok()
                .filter(|value| (100..=120000).contains(value))
                .ok_or_else(|| Failure::usage("--timeout-ms must be between 100 and 120000"))?,
        )
    } else {
        Duration::from_secs(30)
    };
    let mut temporaries = Vec::new();
    let (source_file, before_file, after_file, live_after, before_label, after_label, provenance) =
        if flags.contains_key("--old") || flags.contains_key("--new") {
            if file.is_some()
                || flags.contains_key("--base")
                || flags.contains_key("--head")
                || merge_base
            {
                return Err(Failure::usage(
                    "--old/--new cannot be combined with Git inputs",
                ));
            }
            let old = flags
                .get("--old")
                .ok_or_else(|| Failure::usage("--old and --new are both required"))?;
            let new = flags
                .get("--new")
                .ok_or_else(|| Failure::usage("--old and --new are both required"))?;
            if *old == "/dev/null" && *new == "/dev/null" {
                return Err(Failure::usage("both comparison sides are absent"));
            }
            let identity =
                source::canonical_source(Path::new(if *new == "/dev/null" { old } else { new }))
                    .map_err(Failure::usage)?;
            let before_file = snapshot(explicit_file(old)?, &mut temporaries)?;
            let after_file = snapshot(explicit_file(new)?, &mut temporaries)?;
            (
                identity,
                before_file,
                after_file,
                false,
                old.to_string(),
                new.to_string(),
                json!({"mode": "file_snapshots", "old": old, "new": new}),
            )
        } else {
            let file = file.ok_or_else(|| Failure::usage(HELP))?;
            let base = flags.get("--base").ok_or_else(|| {
                Failure::usage("Git comparisons require an explicit --base revision")
            })?;
            if merge_base && !flags.contains_key("--head") {
                return Err(Failure::usage(
                    "--merge-base requires an explicit --head revision",
                ));
            }
            let repository = GitSource::discover(Path::new(file))
                .map_err(|error| Failure::runtime("GIT_SOURCE", error))?;
            let mut base_commit = repository
                .resolve(base)
                .map_err(|error| Failure::runtime("GIT_REVISION", error))?;
            let head_commit = flags
                .get("--head")
                .map(|head| repository.resolve(head))
                .transpose()
                .map_err(|error| Failure::runtime("GIT_REVISION", error))?;
            if merge_base {
                base_commit = repository
                    .merge_base(
                        &base_commit,
                        head_commit.as_deref().expect("validated head"),
                    )
                    .map_err(|error| Failure::runtime("GIT_MERGE_BASE", error))?;
            }
            let before = repository
                .read(&base_commit)
                .map_err(|error| Failure::runtime("GIT_BLOB", error))?;
            let before_file = snapshot(before, &mut temporaries)?;
            let identity = source::canonical_location(Path::new(file)).map_err(Failure::usage)?;
            let after_file = if let Some(head) = &head_commit {
                snapshot(
                    repository
                        .read(head)
                        .map_err(|error| Failure::runtime("GIT_BLOB", error))?,
                    &mut temporaries,
                )?
            } else {
                Some(identity.to_str().expect("validated path").to_owned())
            };
            let after_label = head_commit
                .clone()
                .unwrap_or_else(|| "working tree (live)".into());
            let provenance = json!({"mode": if merge_base { "merge_base" } else { "exact_base" }, "repository": repository.root, "path": repository.relative, "base_commit": base_commit, "head_commit": head_commit, "live_after": head_commit.is_none()});
            (
                identity,
                before_file,
                after_file,
                head_commit.is_none(),
                base_commit,
                after_label,
                provenance,
            )
        };
    let source_file = source_file
        .to_str()
        .ok_or_else(|| Failure::usage("source path is not UTF-8"))?
        .to_owned();
    let project = resolve_project(&SystemRunner)
        .map_err(|error| Failure::runtime("PROJECT_CONTEXT", error))?;
    let command = Command::OpenComparison {
        project: project.clone(),
        source_file: source_file.clone(),
        before_file,
        after_file,
        live_after,
        before_label,
        after_label,
        highlights,
        old_highlights,
    };
    // Temporaries remain alive until the server has captured its own immutable inputs
    // and the document readiness check has completed, including failure paths.
    let mut result = open_command(
        command,
        project,
        source_file,
        DocumentKind::Comparison,
        None,
        timeout,
    )?;
    result["comparison"] = provenance;
    drop(temporaries);
    Ok(result)
}
