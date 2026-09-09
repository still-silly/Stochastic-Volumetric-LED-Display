use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rustc-check-cfg=cfg(svled_opencv5)");

    // The opencv crate exposes slightly different Rust signatures for OpenCV
    // 4 and 5. Detect the native branch used by the build and let scan.rs
    // select the matching calls. Keep OpenCV 4 as the conservative fallback
    // when pkg-config is unavailable (for example in a no-scan build).
    let version = ["opencv4", "opencv5", "opencv"].iter().find_map(|package| {
        Command::new("pkg-config")
            .args(["--modversion", package])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
    });

    if version
        .as_deref()
        .and_then(|value| value.trim().split('.').next())
        .and_then(|major| major.parse::<u32>().ok())
        == Some(5)
    {
        println!("cargo:rustc-cfg=svled_opencv5");
    }
}
