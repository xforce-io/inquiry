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
    let head_path = git(&["rev-parse", "--git-path", "HEAD"]);
    println!("cargo:rerun-if-changed={head_path}");
    if !reference.is_empty() {
        let ref_path = git(&["rev-parse", "--git-path", &reference]);
        println!("cargo:rerun-if-changed={ref_path}");
    }
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rustc-env=INQUIRY_BUILD_SHA={sha}");
}
