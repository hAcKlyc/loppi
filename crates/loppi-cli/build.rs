fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // The derived command parser and dispatch exceed the MSVC 1 MiB default
        // in unoptimized builds. Reserve the same 8 MiB used by Unix main stacks;
        // Windows commits these pages on demand.
        println!("cargo:rustc-link-arg-bin=loppi=/STACK:8388608");
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/windows-runtime.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
