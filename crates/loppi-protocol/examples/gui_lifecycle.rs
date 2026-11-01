//! Integration driver for the exact GUI IPC/exit path (isolated LOPPI_DATA_DIR).
fn main() {
    let result = if std::env::args().nth(1).as_deref() == Some("update-abort") {
        loppi_protocol::quit_gui_core().and_then(|()| {
            assert!(
                loppi_protocol::send_from(
                    loppi_protocol::ClientKind::Gui,
                    loppi_protocol::Request::Status
                )
                .is_err()
            );
            loppi_protocol::resume_gui_core()?;
            loppi_protocol::send_from(
                loppi_protocol::ClientKind::Gui,
                loppi_protocol::Request::Status,
            )?;
            loppi_protocol::quit_gui_core()
        })
    } else if std::env::args().nth(1).as_deref() == Some("quit") {
        loppi_protocol::quit_gui_core().and_then(|()| {
            assert!(
                loppi_protocol::send_from(
                    loppi_protocol::ClientKind::Gui,
                    loppi_protocol::Request::Status
                )
                .is_err()
            );
            Ok(())
        })
    } else {
        loppi_protocol::send_from(
            loppi_protocol::ClientKind::Gui,
            if std::env::args().nth(1).as_deref() == Some("opened") {
                loppi_protocol::Request::AppOpened
            } else {
                loppi_protocol::Request::Status
            },
        )
        .map(|response| println!("{}", serde_json::to_string(&response).unwrap()))
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
