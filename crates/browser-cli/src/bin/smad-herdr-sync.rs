use smad_cli::herdr::run_herdr_sync;

fn main() {
    if let Err(error) = run_herdr_sync(std::env::args().skip(1).collect()) {
        eprintln!("smad-herdr-sync: {error}");
        std::process::exit(1);
    }
}
