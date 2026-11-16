use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-env-changed=RC");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let resource = output.join("app.res");
    let compiler = env::var_os("RC").unwrap_or_else(|| "rc.exe".into());
    let status = Command::new(compiler)
        .args(["/nologo", "/fo"])
        .arg(&resource)
        .arg("assets/app.rc")
        .status()
        .expect("Windows SDK rc.exe is required. Build from a VS Developer PowerShell or set RC.");
    assert!(status.success(), "Windows icon/resource compilation failed");
    println!(
        "cargo:rustc-link-arg-bin=vibe-voice-lite={}",
        resource.display()
    );
}
