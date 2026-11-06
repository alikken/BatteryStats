use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=res/app.rc.in");
    println!("cargo:rerun-if-changed=res/app.manifest.in");
    println!("cargo:rerun-if-changed=res/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let res = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("res");
    let ver = std::env::var("CARGO_PKG_VERSION").unwrap();
    let v1 = std::env::var("CARGO_PKG_VERSION_MAJOR").unwrap();
    let v2 = std::env::var("CARGO_PKG_VERSION_MINOR").unwrap();
    let v3 = std::env::var("CARGO_PKG_VERSION_PATCH").unwrap();
    let fill = |s: String| {
        s.replace("@V1@", &v1).replace("@V2@", &v2).replace("@V3@", &v3).replace("@VER@", &ver)
    };

    let manifest = fill(std::fs::read_to_string(res.join("app.manifest.in")).unwrap());
    std::fs::write(out_dir.join("app.manifest"), manifest).unwrap();
    std::fs::copy(res.join("app.ico"), out_dir.join("app.ico")).unwrap();
    let rc = fill(std::fs::read_to_string(res.join("app.rc.in")).unwrap()).replace("@RES@/", "");
    std::fs::write(out_dir.join("app.rc"), rc).unwrap();

    let obj = out_dir.join("app_res.o");
    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
    let status = Command::new(windres)
        .current_dir(&out_dir)
        .args(["-c", "65001", "-O", "coff", "-i", "app.rc", "-o"])
        .arg(&obj)
        .status()
        .expect("windres not found");
    assert!(status.success(), "windres failed");
    println!("cargo:rustc-link-arg-bins={}", obj.display());
}
