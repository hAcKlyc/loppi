fn main() {
    println!("cargo:rerun-if-changed=native/system_audio.m");
    println!("cargo:rerun-if-changed=native/permissions.m");
    cc::Build::new()
        .file("native/system_audio.m")
        .file("native/permissions.m")
        .flag("-fobjc-arc")
        .flag("-fblocks")
        .compile("hear_system_audio");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=ScreenCaptureKit");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
}
