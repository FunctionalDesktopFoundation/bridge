use std::path::PathBuf;

fn main() {
    let libs = &["Qt6Core", "Qt6Gui", "Qt6Qml", "Qt6Quick", "Qt6Network"];
    let src_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("src");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let moc_path = pkg_config::Config::new()
        .probe("Qt6Core")
        .ok()
        .and_then(|info| {
            info.link_paths.first().and_then(|lp| {
                let moc = lp.parent()?.join("libexec").join("moc");
                if moc.exists() { Some(moc) } else { None }
            })
        })
        .or_else(|| {
            let root = PathBuf::from("/nix/store");
            std::fs::read_dir(&root)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().contains("qtbase"))
                .find_map(|e| {
                    let moc = e.path().join("libexec").join("moc");
                    if moc.exists() { Some(moc) } else { None }
                })
        })
        .unwrap_or_else(|| PathBuf::from("moc"));

    eprintln!("cargo:warning=using moc at: {}", moc_path.display());

    let moc_out = out_dir.join("moc_trash_bridge.cpp");
    let status = std::process::Command::new(&moc_path)
        .arg(src_dir.join("trash_bridge.h"))
        .arg("-o")
        .arg(&moc_out)
        .status()
        .expect("failed to run moc");
    assert!(status.success(), "moc failed");

    let mut includes: Vec<String> = Vec::new();

    for lib in libs {
        if let Ok(info) = pkg_config::Config::new()
            .atleast_version("6.0")
            .probe(lib)
        {
            for path in &info.include_paths {
                let s = path.display().to_string();
                if !includes.contains(&s) {
                    includes.push(s);
                }
            }
            for lib_name in &info.libs {
                println!("cargo:rustc-link-lib={}", lib_name);
            }
            for link_path in &info.link_paths {
                println!("cargo:rustc-link-search={}", link_path.display());
            }
        } else {
            println!("cargo:rustc-link-lib={}", lib);
        }
    }

    let mut build = cc::Build::new();
    build.cpp(true)
        .flag("-std=c++17")
        .file(src_dir.join("trash_bridge.cpp"))
        .file(&moc_out);
    for inc in &includes {
        build.include(inc);
    }
    build.include(&src_dir);
    build.compile("trash_bridge");
}
