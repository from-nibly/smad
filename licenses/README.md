# Third-party attribution

These are dependency notices, not a change to the licensing of SMAD's own code.

- `merman-0.9.0`: upstream own-code licenses and the pinned `rust-render-base` component notices.
  SMAD compiles native diagram families, SVG and Cytoscape; ELK and math/font features are disabled.
- `syntect-5.3.0`: highlighting implementation and default theme attribution.
- `two-face-0.5.2`: full upstream syntax/theme acknowledgements from the pinned
  `two-face 0.5.2+bat-0.26.1` bundle (only its syntaxes are used).
- `syntaxes`: notices for the five pinned, unmodified community syntax supplements;
  exact revisions and hashes are in `assets/syntaxes/upstream.json`.
  Other supplemental definitions are SMAD-authored, not copied from third-party code.
- The Cargo `onig_sys` attribution includes its vendored Oniguruma `COPYING` file
  as well as the Rust bindings' license.
- `toon-format-0.7.0`: the official Rust TOON encoder's MIT license.
- `cargo-overrides`: pinned license texts missing from published crate archives, with provenance.

`packaging/licenses.py` adds the locked active Cargo dependency inventory and license texts to
release packages. Unmodified MPL-2.0 dependency source is also included there so recipients have
the covered source, not just a link to a third-party service. The pipeline fails when it cannot
find a required attribution text. GTK/WebKit system libraries are dynamically linked and are
not bundled; install them through the host distribution.
