fn main() {
    if std::env::var("CARGO_FEATURE_QT").is_ok() {
        let libs = pkg_config::Config::new().probe("Qt6Core").unwrap();
        for lib in &libs.libs {
            println!("cargo:rustc-link-lib={}", lib);
        }
        for path in &libs.link_paths {
            println!("cargo:rustc-link-search={}", path.display());
        }
        let mut build = cc::Build::new();
        build.cpp(true).file("src/fdf_bridge.cpp");
        for path in &libs.include_paths {
            build.include(path);
        }
        build.compile("fdf_bridge");
    }
}
