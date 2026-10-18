// Link the icon and manifest (assets/app.res, made by scripts/icon.py) into the exe;
// building needs no Windows SDK.
fn main() {
    println!("cargo:rerun-if-changed=assets/app.res");
    println!("cargo:rustc-link-arg-bins={}/assets/app.res", env!("CARGO_MANIFEST_DIR"));
}
