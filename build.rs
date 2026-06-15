fn main() {
    if std::env::var("CARGO_FEATURE_QT").is_ok() {
        if let Ok(libs) = pkg_config::Config::new().probe("Qt6Core") {
            for lib in &libs.libs {
                println!("cargo:rustc-link-lib={}", lib);
            }
            for path in &libs.link_paths {
                println!("cargo:rustc-link-search={}", path.display());
            }
        }
        cc::Build::new()
            .cpp(true)
            .file("src/fdf_bridge.cpp")
            .compile("fdf_bridge");
    }
}
