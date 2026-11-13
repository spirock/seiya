fn main() {
    println!("cargo:rerun-if-changed=src/native_owned_socket.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let object = output.join("native_owned_socket.o");
    let architecture = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        _ => panic!("unsupported macOS architecture for native socket observer"),
    };
    assert!(std::process::Command::new("/usr/bin/cc")
        .args(["-arch", architecture])
        .args([
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-c",
            "src/native_owned_socket.c",
            "-o"
        ])
        .arg(&object)
        .status()
        .expect("macOS SDK compiler required")
        .success());
    assert!(std::process::Command::new("/usr/bin/ar")
        .arg("crs")
        .arg(output.join("libseiya_owned_socket.a"))
        .arg(object)
        .status()
        .expect("macOS SDK archiver required")
        .success());
    println!("cargo:rustc-link-search=native={}", output.display());
    println!("cargo:rustc-link-lib=static=seiya_owned_socket");
}
