use std::{
    collections::BTreeSet,
    env,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
fn git(root: &std::path::Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../..")
        .canonicalize()
        .unwrap();
    // Track source files AND their directories so edits/new files invalidate identity.
    // Never watch target/node_modules, or build output would trigger its own rebuild.
    let mut paths = BTreeSet::new();
    if let Some(files) = git(
        &root,
        &["ls-files", "--cached", "--others", "--exclude-standard"],
    ) {
        for file in files.lines() {
            let path = root.join(file);
            paths.insert(path.clone());
            if let Some(parent) = path.parent().filter(|p| *p != root) {
                paths.insert(parent.to_path_buf());
            }
        }
    }
    for name in ["HEAD", "index", "packed-refs"] {
        if let Some(path) = git(&root, &["rev-parse", "--git-path", name]) {
            paths.insert(root.join(path));
        }
    }
    if let Some(reference) = git(&root, &["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&root, &["rev-parse", "--git-path", &reference]) {
            paths.insert(root.join(path));
        }
    }
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rerun-if-env-changed=LOPPI_BUILD_EPOCH");
    let sha = git(&root, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = git(&root, &["status", "--porcelain"])
        .map(|s| (!s.is_empty()).to_string())
        .unwrap_or_else(|| "unknown".into());
    let epoch = env::var("LOPPI_BUILD_EPOCH").unwrap_or_else(|_| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string()
    });
    let epoch: u64 = epoch
        .parse()
        .expect("LOPPI_BUILD_EPOCH must be Unix seconds");
    println!("cargo:rustc-env=LOPPI_GIT_SHA={sha}");
    println!("cargo:rustc-env=LOPPI_GIT_DIRTY={dirty}");
    println!("cargo:rustc-env=LOPPI_BUILD_EPOCH={epoch}");
}
