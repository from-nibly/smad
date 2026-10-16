fn main() {
    if let Err(error) = smad_cli::run_markdown_open(std::env::args().skip(1).collect()) {
        eprintln!("lavish-md-open: {error}");
        std::process::exit(1);
    }
}
