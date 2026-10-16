mod application;

fn main() -> gtk::glib::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => application::run(),
        [flag] if flag == "--version" || flag == "-V" => {
            println!("smad {}", env!("CARGO_PKG_VERSION"));
            gtk::glib::ExitCode::SUCCESS
        }
        [flag] if flag == "--help" || flag == "-h" => {
            println!(
                "SMAD — Show Me Agent Document\n\nUsage: smad [--help|--version]\n\nStarts the persistent GTK document server/window.\nOpen files with: smad-axi open md://./README.md\nReview HTML with: smad-axi open lavish://./review.html\nInspect without starting: smad-axi status"
            );
            gtk::glib::ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Unknown server argument. Use smad --help or smad-axi open <document-uri>.");
            gtk::glib::ExitCode::from(2)
        }
    }
}
