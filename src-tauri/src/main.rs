// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    if let Err(error) = loppi_protocol::platform::initialize_process() {
        eprintln!("loppi: {error}");
        std::process::exit(1);
    }
    if std::env::args().nth(1).as_deref() == Some("--build-info") {
        println!(
            "{}",
            serde_json::to_string(&loppi_protocol::build_info("gui")).unwrap()
        );
        return;
    }
    loppi_desktop_lib::run()
}
