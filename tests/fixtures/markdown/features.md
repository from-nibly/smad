# SMAD · native rendering

**Markdown, code, diagrams and tables — no embedded browser.**

## A real table

| Feature | Detail | Status |
| :--- | :---: | ---: |
| **Code** | `Rust`, Python, JSON and more | Highlighted |
| *Mermaid* | Native SVG | Rendered |
| [Navigation](next.md#next-steps) | 日本語 · 🌲 | Native links |

## Highlighted code

```rust
fn main() {
    let message = "Show Me Agent Document";
    println!("{message}"); // native syntax colours
}
```

## A native diagram

```mermaid
flowchart LR
    Agent[Agent] -->|md://| SMAD[SMAD]
    SMAD --> Text[Native GTK]
    SMAD --> Code[Highlighting]
    SMAD --> Diagram[Mermaid SVG]
```

## Another diagram family

```mermaid
sequenceDiagram
    Agent->>SMAD: Open md://document.md
    SMAD-->>Agent: Ready + document state
    Note over SMAD: Live updates stay native
```

## Explicit error behavior

This deliberately invalid diagram must show a diagnostic and retain its source:

```mermaid
not-a-supported-diagram this is intentionally invalid
```

Unknown code languages remain readable plain code:

```unregistered-language
some source that should never disappear
```

Raw HTML remains inert: <b>not an HTML renderer</b>.
