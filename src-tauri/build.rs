fn main() {
    println!("cargo:rerun-if-changed=native/agent.m");
    println!("cargo:rerun-if-changed=native/privacy.m");
    cc::Build::new()
        .file("native/agent.m")
        .file("native/privacy.m")
        .flag("-fobjc-arc")
        .compile("hear_agent");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=ServiceManagement");
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    tauri_build::build();
}
