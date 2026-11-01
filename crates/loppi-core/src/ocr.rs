//! A bundled, independent offline OCR child. Never invokes a cloud or system shell.
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
pub fn recognize(image: &Path) -> Result<String, String> {
    let root = std::env::current_exe()
        .map_err(|_| "ocr_bundle_missing")?
        .parent()
        .ok_or("ocr_bundle_missing")?
        .join("resources/ocr");
    recognize_with_root(image, &root)
}

fn recognize_with_root(image: &Path, root: &Path) -> Result<String, String> {
    let engine = root.join("tesseract.exe");
    if !engine.is_file()
        || !root.join("tessdata/chi_sim.traineddata").is_file()
        || !root.join("tessdata/eng.traineddata").is_file()
    {
        return Err("ocr_bundle_missing".into());
    }
    let mut output = tempfile::tempfile_in(image.parent().ok_or("ocr_image_missing")?)
        .map_err(|_| "ocr_output")?;
    let mut child = loppi_protocol::platform::command(engine)
        .arg(image)
        .arg("stdout")
        .arg("--tessdata-dir")
        .arg(root.join("tessdata"))
        .args(["-l", "chi_sim+eng"])
        .env("OMP_THREAD_LIMIT", "2")
        .stdin(Stdio::null())
        .stdout(output.try_clone().map_err(|_| "ocr_output")?)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "ocr_start")?;
    let _lifetime = match loppi_protocol::platform::terminate_child_with_owner(&child) {
        Ok(job) => job,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("ocr_lifetime".into());
        }
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("ocr_timeout".into());
            }
        }
        if output.metadata().map_err(|_| "ocr_output")?.len() > 128 * 1024 {
            let _ = child.kill();
            let _ = child.wait();
            return Err("ocr_output_limit".into());
        }
    };
    if !status.success() {
        return Err("ocr_failed".into());
    }
    output.seek(SeekFrom::Start(0)).map_err(|_| "ocr_output")?;
    let mut text = String::new();
    output
        .take(128 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|_| "ocr_output")?;
    if text.len() > 128 * 1024 {
        return Err("ocr_output_limit".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "explicit synthetic PNG + built offline OCR runtime required"]
    fn recognizes_synthetic_png_with_bundled_engine() {
        let root = std::path::PathBuf::from(
            std::env::var_os("LOPPI_TEST_OCR_RUNTIME").expect("synthetic runtime"),
        );
        let image = std::path::PathBuf::from(
            std::env::var_os("LOPPI_TEST_OCR_PNG").expect("synthetic PNG"),
        );
        let text = super::recognize_with_root(&image, &root).unwrap();
        assert!(
            text.contains("12345") && text.to_uppercase().contains("WINDOWS"),
            "{text}"
        );
    }
}
