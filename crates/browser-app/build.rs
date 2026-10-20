use std::{env, fs, path::PathBuf};
use syntect::parsing::{Regex, SyntaxDefinition, syntax_definition::Pattern};

fn main() {
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../assets/syntaxes");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut paths: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "sublime-syntax"))
        .collect();
    paths.sort();
    let mut builder = two_face::syntax::extra_newlines().into_builder();
    for path in paths {
        let source = fs::read_to_string(&path).unwrap();
        let name = path.file_stem().unwrap().to_str().unwrap();
        let mut definition = SyntaxDefinition::load_from_str(&source, true, Some(name))
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        if name == "CUE" {
            // The YAML loader marks parent-match backrefs only on pop rules. CUE
            // also needs its opening hash count for escapes/interpolation pushes.
            // Enable the parser's existing backref support on those rules too.
            for context in definition.contexts.values_mut() {
                for pattern in &mut context.patterns {
                    if let Pattern::Match(pattern) = pattern
                        && pattern.regex.regex_str().contains(r"\1")
                    {
                        pattern.has_captures = true;
                        context.uses_backrefs = true;
                    }
                }
            }
        }
        builder.add(definition);
    }
    // Syntect lazily compiles patterns and panics on invalid bundled regexes.
    // Reject static regex errors at build time instead of on a user's document.
    for syntax in builder.syntaxes() {
        for context in syntax.contexts.values() {
            for pattern in &context.patterns {
                if let Pattern::Match(pattern) = pattern
                    && !pattern.has_captures
                    && let Some(error) = Regex::try_compile(pattern.regex.regex_str())
                {
                    panic!("Invalid regex in {}: {error}", syntax.name);
                }
            }
        }
        if let Some(first) = &syntax.first_line_match
            && let Some(error) = Regex::try_compile(first)
        {
            panic!("Invalid first-line regex in {}: {error}", syntax.name);
        }
    }
    // Link and serialize at build time, not during the first native document render.
    let syntaxes = builder.build();
    let baseline = two_face::syntax::extra_newlines().find_unlinked_contexts();
    let unresolved = syntaxes.find_unlinked_contexts();
    let new: Vec<_> = unresolved.difference(&baseline).collect();
    assert!(
        new.is_empty(),
        "Unresolved supplemental grammar contexts: {new:?}"
    );
    syntect::dumps::dump_to_file(
        &syntaxes,
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("syntaxes.packdump"),
    )
    .unwrap();
}
