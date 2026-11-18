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
    let version = env!("CARGO_PKG_VERSION");
    let numeric = [
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH"),
        "0",
    ]
    .join(",");
    let template = std::fs::read_to_string("assets/app.rc").expect("Windows resource template");
    let rc = output.join("app.rc");
    std::fs::write(
        &rc,
        template
            .replace("VIBE_VERSION_NUM", &numeric)
            .replace("VIBE_VERSION_STR", &format!("\"{version}\"")),
    )
    .expect("write versioned resource");
    let status = Command::new(compiler)
        .args(["/nologo", "/fo"])
        .arg(&resource)
        .arg(&rc)
        .status()
        .expect("Windows SDK rc.exe is required. Build from a VS Developer PowerShell or set RC.");
    assert!(status.success(), "Windows icon/resource compilation failed");
    println!(
        "cargo:rustc-link-arg-bin=vibe-voice-lite={}",
        resource.display()
    );
}
