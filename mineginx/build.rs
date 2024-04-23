use std::process::Command;
use std::str;

fn main() {
    let git_tag = Command::new("git")
        .arg("tag")
        .arg("-l")
        .arg("v*")
        .output()
        .expect("required git to get version to compile");
    let git_rev = Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .expect("required git to get version to compile");
    let tag = str::from_utf8(&git_tag.stdout).expect("failed to parse git output");
    let version = tag.split('\n').into_iter().find(|x| !x.is_empty()).unwrap_or("undefined");
    let hash = str::from_utf8(&git_rev.stdout[0..10]).expect("failed to parse git output");
    println!("cargo:rustc-env=MINEGINX_VERSION={}", version);
    println!("cargo:rustc-env=MINEGINX_HASH={}", hash);
}
