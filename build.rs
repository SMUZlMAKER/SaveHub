fn main() {
    println!("cargo:rerun-if-env-changed=SAVEHUB_UPDATER_PUBLIC_KEY");
    tauri_build::build()
}
