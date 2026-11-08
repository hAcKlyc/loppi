fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Unoptimized Clap command factories nest 687,304 and 373,944 byte
        // frames, exceeding the MSVC 1 MiB default before command dispatch.
        // Reserve 2 MiB for this CLI only; pages are committed on demand.
        println!("cargo:rustc-link-arg-bin=loppi=/STACK:2097152");
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/windows-runtime.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
