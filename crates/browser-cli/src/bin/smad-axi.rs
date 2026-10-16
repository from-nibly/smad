fn main() {
    std::process::exit(smad_cli::axi::run(std::env::args().skip(1).collect()));
}
