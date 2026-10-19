//! Integration driver for the exact GUI IPC/exit path (isolated HEAR_DATA_DIR).
fn main() {
    let result = if std::env::args().nth(1).as_deref() == Some("quit") {
        hear_protocol::quit_gui_core().and_then(|()| {
            assert!(
                hear_protocol::send_from(
                    hear_protocol::ClientKind::Gui,
                    hear_protocol::Request::Status
                )
                .is_err()
            );
            Ok(())
        })
    } else {
        hear_protocol::send_from(
            hear_protocol::ClientKind::Gui,
            if std::env::args().nth(1).as_deref() == Some("opened") {
                hear_protocol::Request::AppOpened
            } else {
                hear_protocol::Request::Status
            },
        )
        .map(|response| println!("{}", serde_json::to_string(&response).unwrap()))
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
