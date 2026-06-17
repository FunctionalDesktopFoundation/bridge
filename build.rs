fn main() {
    if std::env::var("CARGO_FEATURE_QT").is_ok() {
        let qt_modules = ["Qt6Core", "Qt6Gui", "Qt6Qml", "Qt6Widgets"];
        let mut build = cc::Build::new();
        build.cpp(true)
            .file("src/fdf_bridge.cpp")
            .file("src/sharedsettings_fallback.cpp");

        let out_dir = std::env::var("OUT_DIR").unwrap();
        let hooks_config = std::path::Path::new(&out_dir).join("hooks_config.h");
        if !hooks_config.exists() {
            std::fs::write(&hooks_config, "// Default empty hooks config\nstatic const HookEntry g_hooks[] = {};\nstatic const int g_hookCount = 0;\n").unwrap();
        }
        build.include(&out_dir);

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
        let moc_bridge = std::path::Path::new(&out_dir).join("moc_fdf_bridge.cpp");
        let status = std::process::Command::new(&moc_path)
            .args(["-o", &moc_bridge.to_string_lossy(), "src/fdf_bridge.h"])
            .status()
            .expect("failed to run moc on fdf_bridge.h");
        assert!(status.success(), "moc processing of fdf_bridge.h failed");
        build.file(&moc_bridge);

        let moc_settings = std::path::Path::new(&out_dir).join("moc_sharedsettings.cpp");
        let status = std::process::Command::new(&moc_path)
            .args(["-o", &moc_settings.to_string_lossy(), "src/sharedsettings.h"])
            .status()
            .expect("failed to run moc on sharedsettings.h");
        assert!(status.success(), "moc processing of sharedsettings.h failed");
        build.file(&moc_settings);

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
