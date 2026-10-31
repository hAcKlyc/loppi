fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/windows-runtime.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rerun-if-changed=native/windows.cpp");
        cc::Build::new()
            .cpp(true)
            .file("native/windows.cpp")
            .flag("/std:c++17")
            .define("_WIN32_WINNT", "0x0A00")
            .compile("loppi_windows");
        for library in [
            "ole32",
            "oleaut32",
            "mmdevapi",
            "uuid",
            "psapi",
            "advapi32",
            "powrprof",
            "windowscodecs",
            "gdi32",
            "user32",
            "dwmapi",
            "wtsapi32",
        ] {
            println!("cargo:rustc-link-lib={library}");
        }
        return;
    }
    println!("cargo:rerun-if-changed=native/schedule.m");
    println!("cargo:rerun-if-changed=native/system_audio.m");
    println!("cargo:rerun-if-changed=native/system_audio_tap.m");
    println!("cargo:rerun-if-changed=native/permissions.m");
    println!("cargo:rerun-if-changed=native/resources.m");
    println!("cargo:rerun-if-changed=native/audio_exclusions.h");
    println!("cargo:rerun-if-changed=native/moments.m");
    cc::Build::new()
        .file("native/system_audio.m")
        .file("native/schedule.m")
        .file("native/system_audio_tap.m")
        .file("native/permissions.m")
        .file("native/resources.m")
        .file("native/moments.m")
        .flag("-fobjc-arc")
        .flag("-fblocks")
        .flag("-mmacosx-version-min=13.0")
        .compile("loppi_system_audio");
    println!("cargo:rustc-link-arg=-mmacosx-version-min=13.0");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=IOKit");
    println!("cargo:rustc-link-lib=framework=ScreenCaptureKit");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=CoreAudio");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=ImageIO");
    println!("cargo:rustc-link-lib=framework=CoreImage");
    println!("cargo:rustc-link-lib=framework=Vision");
}
