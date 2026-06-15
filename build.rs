fn main() {
    if std::env::var("CARGO_FEATURE_QT").is_ok() {
        let qt_modules = ["Qt6Core", "Qt6Gui", "Qt6Qml"];
        let mut build = cc::Build::new();
        build.cpp(true).file("src/fdf_bridge.cpp");

        let mut moc_path = String::from("moc");
        for var in ["libexecdir", "host_bins"] {
            if let Ok(output) = std::process::Command::new("pkg-config")
                .args(["--variable", var, "Qt6Core"])
                .output()
            {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    let candidate = format!("{}/moc", path);
                    if std::path::Path::new(&candidate).exists() {
                        moc_path = candidate;
                        break;
                    }
                }
            }
        }

        let out_dir = std::env::var("OUT_DIR").unwrap();
        let moc_output = std::path::Path::new(&out_dir).join("moc_fdf_bridge.cpp");
        let status = std::process::Command::new(&moc_path)
            .args(["-o", &moc_output.to_string_lossy(), "src/fdf_bridge.h"])
            .status()
            .expect("failed to run moc");
        assert!(status.success(), "moc processing failed");
        build.file(moc_output);

        for module in &qt_modules {
            let libs = pkg_config::Config::new().probe(module).unwrap();
            for lib in &libs.libs {
                println!("cargo:rustc-link-lib={}", lib);
            }
            for path in &libs.link_paths {
                println!("cargo:rustc-link-search={}", path.display());
            }
            for path in &libs.include_paths {
                build.include(path);
            }
        }
        build.compile("fdf_bridge");
    }
}
