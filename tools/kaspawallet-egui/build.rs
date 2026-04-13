use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing manifest dir"));
    let repo_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .expect("crate should live two levels below the repo root")
        .to_path_buf();

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    let backend_bin = out_dir.join(if cfg!(windows) {
        "kaspawallet-gui-backend.exe"
    } else {
        "kaspawallet-gui-backend"
    });

    let status = Command::new("go")
        .env("GOCACHE", out_dir.join("go-cache"))
        .arg("build")
        .arg("-o")
        .arg(&backend_bin)
        .arg("./cmd/kaspawallet-gui-backend")
        .current_dir(&repo_root)
        .status()
        .expect("failed to invoke go build for the GUI backend");

    if !status.success() {
        panic!("go build for cmd/kaspawallet-gui-backend failed");
    }

    println!(
        "cargo:rustc-env=KASPAWALLET_GUI_BACKEND_BIN={}",
        backend_bin.display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("cmd/kaspawallet-gui-backend").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("cmd/kaspawallet").display()
    );
}
