fn main() {
    // D6: tauri.conf.json bundles `../target/pkg-licenses/`, which only
    // `packaging/package-linux.sh` fills (it deletes and restages it). A plain
    // development `cargo build` on a fresh checkout must not fail on its
    // absence, so create it empty here; packaging still replaces it whole.
    let lic = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/pkg-licenses");
    if !lic.exists() {
        let _ = std::fs::create_dir_all(&lic);
    }
    tauri_build::build()
}
