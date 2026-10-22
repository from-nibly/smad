//! One bounded background renderer shared by native documents; no browser or JS runtime.
use merman::{OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};
use pulldown_cmark::{CodeBlockKind, Event, Tag, TagEnd};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, LazyLock, mpsc};
use std::time::Duration;

pub type Diagrams = HashMap<String, Result<Arc<str>, String>>;
const MAX_DIAGRAMS: usize = 8;
const MAX_SOURCE: usize = 32 * 1024;
const MAX_SVG: usize = 2 * 1024 * 1024;
const CACHE_BYTES: usize = 8 * 1024 * 1024;

struct Job {
    content: String,
    control: OperationControl,
    reply: mpsc::SyncSender<Result<Diagrams, String>>,
}

static WORKER: LazyLock<mpsc::SyncSender<Job>> = LazyLock::new(|| {
    let (sender, receiver) = mpsc::sync_channel::<Job>(8);
    std::thread::Builder::new()
        .name("smad-mermaid".into())
        .spawn(move || {
            let mut cache: VecDeque<(String, Arc<str>)> = VecDeque::new();
            for job in receiver {
                if job.control.is_cancelled() {
                    continue;
                }
                job.control.set_deadline(Duration::from_secs(4));
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut diagrams = HashMap::new();
                    for (index, source) in sources(&job.content).into_iter().enumerate() {
                        job.control.checkpoint().map_err(|e| e.to_string())?;
                        let svg = if index >= MAX_DIAGRAMS {
                            Err("document limit: eight Mermaid diagrams".into())
                        } else if source.len() > MAX_SOURCE {
                            Err("Mermaid source exceeds 32 KiB".into())
                        } else if let Some(position) =
                            cache.iter().position(|(key, _)| key == &source)
                        {
                            let item = cache.remove(position).expect("cache position");
                            let svg = item.1.clone();
                            cache.push_back(item);
                            Ok(svg)
                        } else {
                            let svg = render_svg(&source, job.control.child());
                            if let Ok(svg) = &svg {
                                while !cache.is_empty()
                                    && (cache.len() >= 16
                                        || cache
                                            .iter()
                                            .map(|(s, svg)| s.len() + svg.len())
                                            .sum::<usize>()
                                            + source.len()
                                            + svg.len()
                                            > CACHE_BYTES)
                                {
                                    cache.pop_front();
                                }
                                cache.push_back((source.clone(), svg.clone()));
                            }
                            svg
                        };
                        diagrams.insert(source, svg);
                    }
                    Ok(diagrams)
                }))
                .unwrap_or_else(|_| {
                    Err("Mermaid renderer panicked; previous document retained".into())
                });
                if !job.control.is_cancelled() {
                    let _ = job.reply.send(result);
                }
            }
        })
        .expect("start bounded Mermaid worker");
    sender
});

pub fn prepare(
    content: &str,
) -> Result<(OperationControl, mpsc::Receiver<Result<Diagrams, String>>), String> {
    let control = OperationControl::new();
    let (reply, receiver) = mpsc::sync_channel(1);
    WORKER
        .try_send(Job {
            content: content.into(),
            control: control.clone(),
            reply,
        })
        .map_err(|_| "Mermaid renderer queue is busy; retry Reload".to_owned())?;
    Ok((control, receiver))
}

pub fn sources(content: &str) -> Vec<String> {
    let mut sources = Vec::new();
    let mut current = None;
    for (event, _) in super::markdown_parser::events(content).take(20_001) {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(language)))
                if language.split_whitespace().next() == Some("mermaid") =>
            {
                current = Some(String::new())
            }
            Event::Text(text) if current.is_some() => {
                current.as_mut().expect("code block").push_str(&text)
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(source) = current.take() {
                    sources.push(source);
                }
            }
            _ => {}
        }
    }
    sources
}

fn render_svg(source: &str, control: OperationControl) -> Result<Arc<str>, String> {
    if source.len() > MAX_SOURCE {
        return Err("Mermaid source exceeds 32 KiB".into());
    }
    let request = SvgRequest {
        pipeline: Some(merman::svg::SvgPipeline::resvg_safe()),
        ..SvgRequest::default()
    };
    let output = Renderer::new()
        .render(RenderRequest::svg(
            source,
            control.with_deadline(Duration::from_secs(2)),
            request,
        ))
        .map_err(|error| format!("Mermaid: {error}"))?;
    match output {
        RenderOutput::Svg(Some(svg)) if svg.svg().len() <= MAX_SVG => Ok(Arc::from(svg.svg())),
        RenderOutput::Svg(Some(_)) => Err("Mermaid SVG exceeds 2 MiB".into()),
        _ => Err("No supported Mermaid diagram detected".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_diagram_corpus_and_errors() {
        for source in [
            "flowchart LR\n A[Start] --> B[Done]",
            "sequenceDiagram\n Alice->>Bob: Hello\n Bob-->>Alice: Hi",
            "classDiagram\n Animal <|-- Duck\n Animal : +int age",
            "stateDiagram-v2\n [*] --> Ready\n Ready --> Done",
            "erDiagram\n CUSTOMER ||--o{ ORDER : places",
            "pie title Pets\n \"Cats\" : 3\n \"Dogs\" : 2",
        ] {
            let svg = render_svg(source, OperationControl::new())
                .unwrap_or_else(|e| panic!("{source}: {e}"));
            assert!(svg.contains("<svg"));
            assert!(!svg.contains("<foreignObject"));
            assert!(svg.len() < MAX_SVG);
        }
        assert!(render_svg("not a diagram", OperationControl::new()).is_err());
        let cancelled = OperationControl::new();
        cancelled.cancel();
        assert!(render_svg("flowchart LR\n A --> B", cancelled).is_err());
    }
    #[test]
    fn metadata_does_not_prepare_embedded_mermaid_fences() {
        assert_eq!(
            sources(
                "---\nexample: |\n  ```mermaid\n  flowchart LR\n   WRONG --> WRONG\n  ```\n---\n\n```mermaid\nflowchart LR\n A --> B\n```\n"
            ),
            vec!["flowchart LR\n A --> B\n"]
        );
    }
    #[test]
    fn only_mermaid_fences_are_prepared() {
        assert_eq!(
            sources("```rust\nfn main() {}\n```\n```mermaid\nflowchart LR\n A --> B\n```"),
            vec!["flowchart LR\n A --> B\n"]
        );
    }
}
