// Help the linker find libmpv on Windows MSVC portable builds.
// package-windows.ps1 sets KALICUT_MPV_DIR to the folder with mpv.lib + libmpv-2.dll.
// libmpv2-sys already emits `cargo:rustc-link-lib=mpv`.

fn main() {
    println!("cargo:rerun-if-env-changed=KALICUT_MPV_DIR");
    if let Ok(dir) = std::env::var("KALICUT_MPV_DIR") {
        if !dir.is_empty() {
            println!("cargo:rustc-link-search=native={dir}");
        }
    }
}
