use std::process::Command;
fn main() {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    let sha = git(&["rev-parse", "HEAD"]);
    let reference = git(&["symbolic-ref", "HEAD"]);
    println!("cargo:rerun-if-changed=.git/HEAD");
    if !reference.is_empty() {
        println!("cargo:rerun-if-changed=.git/{reference}");
    }
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rustc-env=INQUIRY_BUILD_SHA={sha}");
}
