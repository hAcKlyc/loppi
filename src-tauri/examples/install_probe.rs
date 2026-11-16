use tauri::Manager;
use tauri_plugin_updater::UpdaterExt;

fn main() {
    let executable = std::path::PathBuf::from(std::env::var_os("LOPPI_PROBE_EXECUTABLE").unwrap());
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let handle = app.handle().clone();
            let updater = handle
                .updater_builder()
                .executable_path(&executable)
                .timeout(std::time::Duration::from_secs(15))
                .on_before_exit(|| {})
                .build()?;
            tauri::async_runtime::spawn(async move {
                let result = async {
                    let update = updater.check().await?.expect("synthetic update is newer");
                    let bytes = update.download(|_, _| {}, || {}).await?;
                    let mut tampered = bytes.clone();
                    tampered.push(0);
                    assert!(update.install(&tampered).is_err(), "tampered update was accepted");
                    println!("tampered signed update rejected");
                    std::io::Write::flush(&mut std::io::stdout()).unwrap();
                    update.install(&bytes)
                }.await;
                let exit = match result {
                    Ok(()) => 0,
                    Err(error) => {
                        eprintln!("verified installation failed: {error:?}");
                        1
                    }
                };
                handle.exit(exit);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .unwrap();
}
