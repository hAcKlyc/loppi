mod build_theme;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=../src/tokens.css");
        let theme = include_str!("../src/tokens.css");
        for (name, token) in [("CANVAS", "--color-canvas"), ("TEXT", "--color-text")] {
            println!(
                "cargo:rustc-env=LOPPI_WINDOW_{name}={}",
                build_theme::colorref(theme, token)
            );
        }
        println!("cargo:rerun-if-changed=native/windows.cpp");
        cc::Build::new()
            .cpp(true)
            .file("native/windows.cpp")
            .flag("/std:c++17")
            .define("UNICODE", None)
            .define("_UNICODE", None)
            .compile("loppi_windows_host");
        for library in [
            "user32", "gdi32", "shell32", "advapi32", "winmm", "comctl32", "dwmapi", "wtsapi32",
        ] {
            println!("cargo:rustc-link-lib={library}");
        }
        println!("cargo:rerun-if-changed=../config/windows-runtime.manifest");
        tauri_build::try_build(
            tauri_build::Attributes::new().windows_attributes(
                tauri_build::WindowsAttributes::new()
                    .app_manifest(include_str!("../config/windows-runtime.manifest")),
            ),
        )
        .expect("Windows app resources");
        return;
    }
    println!("cargo:rerun-if-changed=native/agent.m");
    println!("cargo:rerun-if-changed=native/privacy.m");
    println!("cargo:rerun-if-changed=native/entry.m");
    println!("cargo:rerun-if-changed=native/playback.m");
    cc::Build::new()
        .file("native/agent.m")
        .file("native/privacy.m")
        .file("native/entry.m")
        .file("native/playback.m")
        .flag("-fblocks")
        .flag("-mmacosx-version-min=13.0")
        .flag("-fobjc-arc")
        .compile("loppi_agent");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=ServiceManagement");
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=QuartzCore");
    tauri_build::build();
}
