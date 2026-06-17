use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{FdfConfig, Features};

const BRIDGE_SRC: &str = env!("CARGO_MANIFEST_DIR");

#[derive(Debug, Clone, PartialEq)]
pub enum Platform {
    Desktop,
    Windows,
    Android,
    Ios,
    Wasm,
}

impl Platform {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "desktop" => Some(Self::Desktop),
            "windows" => Some(Self::Windows),
            "android" => Some(Self::Android),
            "ios" => Some(Self::Ios),
            "wasm" => Some(Self::Wasm),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Windows => "windows",
            Self::Android => "android",
            Self::Ios => "ios",
            Self::Wasm => "wasm",
        }
    }

    pub fn build_dir(&self) -> &'static str {
        match self {
            Self::Desktop => "build-desktop",
            Self::Windows => "build-windows",
            Self::Android => "build-android",
            Self::Ios => "build-ios",
            Self::Wasm => "build-wasm",
        }
    }

    pub fn is_desktop(&self) -> bool {
        matches!(self, Self::Desktop | Self::Windows)
    }

    pub fn supports_ipc(&self) -> bool {
        matches!(self, Self::Desktop | Self::Windows)
    }

    pub fn supports_window_controls(&self) -> bool {
        matches!(self, Self::Desktop)
    }

    pub fn is_desktop_linux(&self) -> bool {
        matches!(self, Self::Desktop)
    }

    pub fn is_desktop_windows(&self) -> bool {
        matches!(self, Self::Windows)
    }
}

#[derive(Clone)]
pub struct BuildCtx<'a> {
    pub platform: Platform,
    pub output: &'a Path,
    pub app_name: &'a str,
    pub transpiled_qml: &'a str,
    pub build_dir_override: Option<&'a Path>,
    pub config: &'a FdfConfig,
    pub features: Features,
    pub hooks_cpp: Option<&'a str>,
}

impl<'a> BuildCtx<'a> {
    pub fn build_dir(&self) -> PathBuf {
        match self.build_dir_override {
            Some(d) => d.to_path_buf(),
            None => self.output.join(self.platform.build_dir()),
        }
    }
}

pub fn generate_project(ctx: &BuildCtx) -> Result<PathBuf, String> {
    match ctx.platform {
        Platform::Desktop => generate_desktop(ctx),
        Platform::Windows => generate_windows_cmake(ctx),
        Platform::Android => generate_platform_project(ctx, "android"),
        Platform::Ios => generate_platform_project(ctx, "ios"),
        Platform::Wasm => generate_wasm_project(ctx),
    }
}

fn generate_desktop(ctx: &BuildCtx) -> Result<PathBuf, String> {
    let tmp_dir = std::env::temp_dir().join(format!("fdf-build-{}", ctx.app_name));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(tmp_dir.join("src"))
        .map_err(|e| format!("failed to create temp dir: {}", e))?;

    let cargo_toml = format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
fdf-bridge = {{ path = "{}", features = ["qt"] }}
"#,
        ctx.app_name, BRIDGE_SRC
    );
    std::fs::write(tmp_dir.join("Cargo.toml"), &cargo_toml)
        .map_err(|e| format!("failed to write Cargo.toml: {}", e))?;

    let mut main_rs = String::from("fn main() {\n");
    let ffi_file = ctx.output.join("ffi.json");
    if ffi_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&ffi_file) {
            if let Ok(def) = serde_json::from_str::<crate::FfiDefinition>(&content) {
                for f in &def.fns {
                    let fn_name = &f.name;
                    main_rs.push_str(&format!(
                        "    fdf_bridge::ffi::register(\"{}\", Box::new(|args| -> String {{\n",
                        fn_name.replace('\\', "\\\\").replace('"', "\\\"")
                    ));
                    for line in f.code.lines() {
                        main_rs.push_str(&format!("        {}\n", line));
                    }
                    main_rs.push_str("        \"ok\".to_string()\n");
                    main_rs.push_str("    }));\n");
                }
            }
        }
    }
    main_rs.push_str("    fdf_bridge::run_app();\n");
    main_rs.push_str("}\n");
    std::fs::write(tmp_dir.join("src/main.rs"), &main_rs)
        .map_err(|e| format!("failed to write main.rs: {}", e))?;

    let status = Command::new("cargo")
        .args(["generate-lockfile"])
        .current_dir(&tmp_dir)
        .status()
        .map_err(|e| format!("cargo not available: {}", e))?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err("cargo generate-lockfile failed".to_string());
    }

    println!("  Compiling {}...", ctx.app_name);
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&tmp_dir)
        .status()
        .map_err(|e| format!("cargo not available: {}", e))?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err("compilation failed".to_string());
    }

    let binary = tmp_dir.join("target/release").join(ctx.app_name);
    if !binary.exists() {
        let binary = tmp_dir.join("target/release").join(ctx.app_name);
        if !binary.exists() {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err("binary not found after build".to_string());
        }
    }
    let dest = ctx.build_dir().join(ctx.app_name);
    std::fs::create_dir_all(dest.parent().unwrap()).ok();
    std::fs::copy(&binary, &dest).map_err(|e| format!("failed to copy binary: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(dest)
}

fn generate_windows_cmake(ctx: &BuildCtx) -> Result<PathBuf, String> {
    let out = ctx.output;
    let app_name = ctx.app_name;
    let build_dir = ctx.build_dir();
    let src_dir = build_dir.join("src");

    std::fs::create_dir_all(&src_dir)
        .map_err(|e| format!("failed to create {}: {}", src_dir.display(), e))?;

    let bridge_src_dir = Path::new(BRIDGE_SRC).join("src");
    for file in &["fdf_bridge.cpp", "fdf_bridge.h", "sharedsettings.h", "sharedsettings_fallback.cpp"] {
        let src = bridge_src_dir.join(file);
        if src.exists() {
            let _ = std::fs::copy(&src, src_dir.join(file));
        }
    }

    write_hooks_config(&src_dir, ctx.hooks_cpp);

    let main_cpp = r#"#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QDir>
#include <QUrl>
#include <QFont>
#include "fdf_bridge.h"
#include "sharedsettings.h"
#include "hooks_config.h"

int main(int argc, char *argv[]) {
    QGuiApplication app(argc, argv);
    QQuickStyle::setStyle("Fusion");
    QFont defaultFont = app.font();
    defaultFont.setPointSize(10);
    app.setFont(defaultFont);

    qmlRegisterType<SharedSettings>("FDF.SharedSettings", 1, 0, "SharedSettings");

    QQmlApplicationEngine engine;
    engine.addImportPath("qrc:///");

    FDFBridge bridge;
    engine.rootContext()->setContextProperty("bridge", &bridge);

    FDFSettings settings;
    engine.rootContext()->setContextProperty("FDFSettings", &settings);

    FDFPlatform platform;
    engine.rootContext()->setContextProperty("FDFPlatform", &platform);

    FDFFFI ffi;
    engine.rootContext()->setContextProperty("FDFFFI", &ffi);

    FDFIPC ipc;
    engine.rootContext()->setContextProperty("FDFIPC", &ipc);

    FDFClipboard clipboard;
    engine.rootContext()->setContextProperty("FDFClipboard", &clipboard);

    SharedSettings sharedSettings;
    engine.rootContext()->setContextProperty("SharedSettings", &sharedSettings);

    FDFHooks hooks(g_hooks, g_hookCount);
    engine.rootContext()->setContextProperty("FDFHooks", &hooks);

    engine.rootContext()->setContextProperty("FDF_IS_MOBILE", false);
    engine.rootContext()->setContextProperty("FDF_HAVE_WINDOW_CONTROLS", false);
    engine.rootContext()->setContextProperty("FDF_PLATFORM", "windows");
    engine.rootContext()->setContextProperty("FDF_TOUCH_TARGET", 32);
    engine.rootContext()->setContextProperty("FDF_DARK_MODE", sharedSettings.darkMode());
    engine.rootContext()->setContextProperty("FDF_ACCENT_COLOR", sharedSettings.accentColor());
    engine.rootContext()->setContextProperty("FDF_THEME_NAME", sharedSettings.themeName());

    QUrl url("qrc:///shell.qml");
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed,
        &app, [&]() {
        qWarning("Failed to load QML: %s", qPrintable(url.toString()));
        QCoreApplication::exit(1);
    });

    engine.load(url);
    return app.exec();
}
"#;
    std::fs::write(src_dir.join("main.cpp"), main_cpp)
        .map_err(|e| format!("failed to write main.cpp: {}", e))?;

    let cmake = format!(
        r#"cmake_minimum_required(VERSION 3.16)
project({app} VERSION 1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)

find_package(Qt6 REQUIRED COMPONENTS Core Gui Qml Quick)

qt_add_executable({app}
    src/main.cpp
    src/fdf_bridge.cpp
    src/sharedsettings.h
    src/sharedsettings_fallback.cpp
    resources.qrc
)

target_link_libraries({app} PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Qml
    Qt6::Quick
)

target_include_directories({app} PRIVATE src)
"#,
        app = app_name,
    );
    std::fs::write(build_dir.join("CMakeLists.txt"), &cmake)
        .map_err(|e| format!("failed to write CMakeLists.txt: {}", e))?;

    let qml_asset_dir = build_dir.join("qml");
    std::fs::create_dir_all(&qml_asset_dir).ok();

    let shell_qml_src = out.join("shell.qml");
    if shell_qml_src.exists() {
        std::fs::copy(&shell_qml_src, qml_asset_dir.join("shell.qml")).ok();
    }

    for asset in &["FDF", "stdlib"] {
        let src = out.join(asset);
        let dst = qml_asset_dir.join(asset);
        if src.exists() {
            std::fs::create_dir_all(&dst).ok();
            copy_dir_recursive(&src, &dst);
        }
    }

    write_shared_config_qml(&build_dir);
    write_resources_qrc(&build_dir);

    write_build_script(&build_dir, app_name, "windows");

    println!("  CMake project generated in {}", build_dir.display());
    println!("  Build on Windows:");
    println!("    cd {} && cmake -B . -G Ninja && cmake --build .", build_dir.display());
    println!("    windeployqt --release {}.exe", app_name);

    Ok(src_dir.join("main.cpp"))
}

fn generate_wasm_project(ctx: &BuildCtx) -> Result<PathBuf, String> {
    let out = ctx.output;
    let app_name = ctx.app_name;
    let build_dir = ctx.build_dir();
    let src_dir = build_dir.join("src");

    std::fs::create_dir_all(&src_dir)
        .map_err(|e| format!("failed to create {}: {}", src_dir.display(), e))?;

    let bridge_src_dir = Path::new(BRIDGE_SRC).join("src");
    for file in &["fdf_bridge.cpp", "fdf_bridge.h", "sharedsettings.h", "sharedsettings_fallback.cpp"] {
        let src = bridge_src_dir.join(file);
        if src.exists() {
            let _ = std::fs::copy(&src, src_dir.join(file));
        }
    }

    write_hooks_config(&src_dir, ctx.hooks_cpp);

    let main_cpp = r#"#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QDir>
#include <QUrl>
#include <QFont>
#include "fdf_bridge.h"
#include "sharedsettings.h"
#include "hooks_config.h"

int main(int argc, char *argv[]) {
    QGuiApplication app(argc, argv);
    QQuickStyle::setStyle("Fusion");
    QFont defaultFont = app.font();
    defaultFont.setPointSize(10);
    app.setFont(defaultFont);

    qmlRegisterType<SharedSettings>("FDF.SharedSettings", 1, 0, "SharedSettings");

    QQmlApplicationEngine engine;
    engine.addImportPath("qrc:///");

    FDFBridge bridge;
    engine.rootContext()->setContextProperty("bridge", &bridge);

    FDFSettings settings;
    engine.rootContext()->setContextProperty("FDFSettings", &settings);

    FDFPlatform platform;
    engine.rootContext()->setContextProperty("FDFPlatform", &platform);

    FDFFFI ffi;
    engine.rootContext()->setContextProperty("FDFFFI", &ffi);

    FDFClipboard clipboard;
    engine.rootContext()->setContextProperty("FDFClipboard", &clipboard);

    SharedSettings sharedSettings;
    engine.rootContext()->setContextProperty("SharedSettings", &sharedSettings);

    FDFHooks hooks(g_hooks, g_hookCount);
    engine.rootContext()->setContextProperty("FDFHooks", &hooks);

    engine.rootContext()->setContextProperty("FDF_IS_MOBILE", false);
    engine.rootContext()->setContextProperty("FDF_HAVE_WINDOW_CONTROLS", false);
    engine.rootContext()->setContextProperty("FDF_PLATFORM", "wasm");
    engine.rootContext()->setContextProperty("FDF_TOUCH_TARGET", 32);
    engine.rootContext()->setContextProperty("FDF_DARK_MODE", sharedSettings.darkMode());
    engine.rootContext()->setContextProperty("FDF_ACCENT_COLOR", sharedSettings.accentColor());
    engine.rootContext()->setContextProperty("FDF_THEME_NAME", sharedSettings.themeName());

    QUrl url("qrc:///shell.qml");
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed,
        &app, [&]() {
        qWarning("Failed to load QML: %s", qPrintable(url.toString()));
        QCoreApplication::exit(1);
    });

    engine.load(url);
    return app.exec();
}
"#;
    std::fs::write(src_dir.join("main.cpp"), main_cpp)
        .map_err(|e| format!("failed to write main.cpp: {}", e))?;

    let cmake = format!(
        r#"cmake_minimum_required(VERSION 3.16)
project({app} VERSION 0.1 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)

find_package(Qt6 REQUIRED COMPONENTS Core Gui Quick)

qt_add_executable({app}
    src/main.cpp
    src/fdf_bridge.cpp
    src/sharedsettings.h
    src/sharedsettings_fallback.cpp
    resources.qrc
)

target_link_libraries({app} PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Quick
)

target_include_directories({app} PRIVATE src)

if(EMSCRIPTEN)
    set_target_properties({app} PROPERTIES
        SUFFIX ".html"
    )
    qt_generate_wasm_app_shell({app})
endif()

qt_finalize_executable({app})
"#,
        app = app_name,
    );
    std::fs::write(build_dir.join("CMakeLists.txt"), &cmake)
        .map_err(|e| format!("failed to write CMakeLists.txt: {}", e))?;

    let qml_asset_dir = build_dir.join("qml");
    std::fs::create_dir_all(&qml_asset_dir).ok();

    let shell_qml_src = out.join("shell.qml");
    if shell_qml_src.exists() {
        std::fs::copy(&shell_qml_src, qml_asset_dir.join("shell.qml")).ok();
    }

    for asset in &["FDF", "stdlib"] {
        let src = out.join(asset);
        let dst = qml_asset_dir.join(asset);
        if src.exists() {
            std::fs::create_dir_all(&dst).ok();
            copy_dir_recursive(&src, &dst);
        }
    }

    write_shared_config_qml(&build_dir);
    write_resources_qrc(&build_dir);

    write_build_script(&build_dir, app_name, "wasm");

    println!("  CMake project generated in {}", build_dir.display());
    println!("  Build with:");
    println!("    emcmake cmake -B {} -G Ninja", build_dir.display());
    println!("    cmake --build {}", build_dir.display());

    Ok(src_dir.join("main.cpp"))
}

fn generate_platform_project(ctx: &BuildCtx, target: &str) -> Result<PathBuf, String> {
    let out = ctx.output;
    let app_name = ctx.app_name;
    let features = &ctx.features;

    let build_dir = ctx.build_dir();
    let src_dir = build_dir.join("src");

    std::fs::create_dir_all(&src_dir)
        .map_err(|e| format!("failed to create {}: {}", src_dir.display(), e))?;

    let bridge_src_dir = Path::new(BRIDGE_SRC).join("src");

    for file in &["fdf_bridge.cpp", "fdf_bridge.h", "sharedsettings.h"] {
        let src = bridge_src_dir.join(file);
        if src.exists() {
            std::fs::copy(&src, src_dir.join(file))
                .map_err(|e| format!("failed to copy {}: {}", file, e))?;
        }
    }

    write_hooks_config(&src_dir, ctx.hooks_cpp);

    let ipc_enabled = features.ipc && ctx.platform.supports_ipc();
    let ffi_enabled = features.ffi;

    let mut ipc_section = String::new();
    if ipc_enabled {
        ipc_section = r#"
    FDFIPC ipc;
    engine.rootContext()->setContextProperty("FDFIPC", &ipc);
"#.to_string();
    }

    let mut ffi_section = String::new();
    if ffi_enabled {
        ffi_section = r#"
    FDFFFI ffi;
    engine.rootContext()->setContextProperty("FDFFFI", &ffi);
"#.to_string();
    }

    let is_mobile = if target == "android" || target == "ios" { "true" } else { "false" };
    let window_controls_flag = if features.window_controls && ctx.platform.supports_window_controls() {
        "true"
    } else {
        "false"
    };

    let main_cpp = format!(
        r#"#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QDir>
#include <QUrl>
#include <QFont>
#include "fdf_bridge.h"
#include "sharedsettings.h"
#include "hooks_config.h"

int main(int argc, char *argv[]) {{
    QGuiApplication app(argc, argv);

    QQuickStyle::setStyle("Material");
    QFont defaultFont = app.font();
    defaultFont.setPointSize(14);
    app.setFont(defaultFont);

    qmlRegisterType<SharedSettings>("FDF.SharedSettings", 1, 0, "SharedSettings");

    QQmlApplicationEngine engine;
    engine.addImportPath("qrc:///");

    FDFBridge bridge;
    engine.rootContext()->setContextProperty("bridge", &bridge);

    FDFSettings settings;
    engine.rootContext()->setContextProperty("FDFSettings", &settings);

    FDFPlatform platform;
    engine.rootContext()->setContextProperty("FDFPlatform", &platform);
{}{}
    FDFClipboard clipboard;
    engine.rootContext()->setContextProperty("FDFClipboard", &clipboard);

    SharedSettings sharedSettings;
    engine.rootContext()->setContextProperty("SharedSettings", &sharedSettings);

    FDFHooks hooks(g_hooks, g_hookCount);
    engine.rootContext()->setContextProperty("FDFHooks", &hooks);

    engine.rootContext()->setContextProperty("FDF_IS_MOBILE", {});
    engine.rootContext()->setContextProperty("FDF_HAVE_WINDOW_CONTROLS", {});
    engine.rootContext()->setContextProperty("FDF_PLATFORM", "{}");
    engine.rootContext()->setContextProperty("FDF_TOUCH_TARGET", 48);
    engine.rootContext()->setContextProperty("FDF_DARK_MODE", sharedSettings.darkMode());
    engine.rootContext()->setContextProperty("FDF_ACCENT_COLOR", sharedSettings.accentColor());
    engine.rootContext()->setContextProperty("FDF_THEME_NAME", sharedSettings.themeName());

    QUrl url("qrc:///shell.qml");
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed,
        &app, [&]() {{
        qWarning("Failed to load QML: %s", qPrintable(url.toString()));
        QCoreApplication::exit(1);
    }});

    engine.load(url);
    return app.exec();
}}
"#,
        ffi_section, ipc_section, is_mobile, window_controls_flag, target
    );
    std::fs::write(src_dir.join("main.cpp"), &main_cpp)
        .map_err(|e| format!("failed to write main.cpp: {}", e))?;

    let qml_asset_dir = build_dir.join("qml");
    std::fs::create_dir_all(&qml_asset_dir).ok();

    let shell_qml_src = out.join("shell.qml");
    if shell_qml_src.exists() {
        std::fs::copy(&shell_qml_src, qml_asset_dir.join("shell.qml")).ok();
    }

    let fdf_asset_src = out.join("FDF");
    if fdf_asset_src.exists() {
        let dst = qml_asset_dir.join("FDF");
        std::fs::create_dir_all(&dst).ok();
        copy_dir_recursive(&fdf_asset_src, &dst);
        if !ctx.platform.supports_window_controls() {
            apply_mobile_qml_fixes(&dst);
        }
    }
    let stdlib_asset_src = out.join("stdlib");
    if stdlib_asset_src.exists() {
        let dst = qml_asset_dir.join("stdlib");
        std::fs::create_dir_all(&dst).ok();
        copy_dir_recursive(&stdlib_asset_src, &dst);
        if !ctx.platform.supports_window_controls() {
            apply_mobile_qml_fixes(&dst);
        }
    }

    write_shared_config_qml(&build_dir);
    write_resources_qrc(&build_dir);

    let android_cfg = ctx.config.android.as_ref();
    let ios_cfg = ctx.config.ios.as_ref();

    match target {
        "android" => {
            let pkg = android_cfg
                .and_then(|c| c.package_name.clone())
                .unwrap_or_else(|| format!("org.fdf.{}", app_name));
            let shared_user_id = format!("com.{}.shared_settings_group", app_name.to_lowercase());
            let min_sdk = android_cfg.map(|c| c.min_sdk).unwrap_or(21);
            let target_sdk = android_cfg.map(|c| c.target_sdk).unwrap_or(33);
            write_android_project(&build_dir, app_name, &pkg, &shared_user_id, min_sdk, target_sdk)?;
        }
        "ios" => {
            let bundle = ios_cfg
                .and_then(|c| c.bundle_id.clone())
                .unwrap_or_else(|| format!("org.fdf.{}", app_name));
            let deployment = ios_cfg
                .map(|c| &c.deployment_target)
                .map(|s| s.as_str())
                .unwrap_or("15.0");
            write_ios_project(&build_dir, app_name, &bundle, deployment)?;
        }
        _ => return Err(format!("unsupported target: {}", target)),
    }

    write_build_script(&build_dir, app_name, target);
    if !ctx.platform.supports_window_controls() {
        write_mobile_config_qml(&build_dir);
    }

    println!("  Project generated in {}", build_dir.display());

    if target == "android" {
        println!("  Build with:");
        println!("    nix develop .#android");
        println!("    cd {} && cmake -B . -G Ninja \\", build_dir.display());
        println!("      -DCMAKE_TOOLCHAIN_FILE=$CMAKE_TOOLCHAIN_FILE \\");
        println!("      -DANDROID_ABI=arm64-v8a");
        println!("    cmake --build .");
    } else if target == "ios" {
        println!("  Build with:");
        println!("    nix develop .#ios");
        println!("    cd {}", build_dir.display());
        println!("    cmake -B . -G Xcode \\");
        println!("      -DCMAKE_SYSTEM_NAME=iOS \\");
        println!("      -DCMAKE_OSX_ARCHITECTURES=arm64");
        println!("    xcodebuild -target {} -configuration Release", app_name);
    }

    Ok(src_dir.join("main.cpp"))
}

fn write_resources_qrc(build_dir: &Path) {
    let qrc = r#"<!DOCTYPE RCC>
<RCC version="1.0">
    <qresource prefix="/">
        <file>qml/shell.qml</file>
    </qresource>
</RCC>
"#;
    let _ = std::fs::write(build_dir.join("resources.qrc"), qrc);
}

fn write_hooks_config(src_dir: &Path, hooks_cpp: Option<&str>) {
    let content = match hooks_cpp {
        Some(c) => c.to_string(),
        None => {
            let mut empty = String::from("// No hooks configured\n");
            empty.push_str("static const HookEntry g_hooks[] = {};\n");
            empty.push_str("static const int g_hookCount = 0;\n");
            empty
        }
    };
    let _ = std::fs::write(src_dir.join("hooks_config.h"), &content);
}

fn apply_mobile_qml_fixes(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                apply_mobile_qml_fixes(&path);
            } else if path.extension().map_or(false, |e| e == "qml") {
                apply_mobile_fixes_to_file(&path);
            }
        }
    }
}

fn apply_mobile_fixes_to_file(path: &Path) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut result = String::with_capacity(content.len() + 256);

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.contains("hoverEnabled") && !trimmed.contains("FDF_IS_MOBILE") {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            result.push_str(&format!("{}hoverEnabled: !FDF_IS_MOBILE\n", indent));
            continue;
        }

        if trimmed.contains("ToolTip.") || (trimmed.contains("ToolTip") && trimmed.contains("{")) {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            result.push_str(&format!("{}visible: !FDF_IS_MOBILE\n", indent));
            result.push_str(line);
            result.push('\n');
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    let _ = std::fs::write(path, &result);
}

fn write_mobile_config_qml(build_dir: &Path) {
    let config = r#"pragma Singleton
import QtQuick

QtObject {
    readonly property bool isMobile: true
    readonly property int touchTargetSize: 48
    readonly property real fontScale: 1.0
    readonly property real spacingScale: 0.85
    readonly property bool hasWindowControls: false
    readonly property bool useSimplifiedAnimations: true
    readonly property real safeAreaTop: 44
    readonly property real safeAreaBottom: 34
}
"#;
    let dir = build_dir.join("qml");
    std::fs::create_dir_all(&dir).ok();
    let _ = std::fs::write(dir.join("MobileConfig.qml"), config);
    let qmldir = "module MobileConfig\nsingleton MobileConfig 1.0 MobileConfig.qml\n";
    let _ = std::fs::write(dir.join("qmldir"), qmldir);
}

fn write_shared_config_qml(build_dir: &Path) {
    let config = r#"pragma Singleton
import QtQuick
import FDF.SharedSettings 1.0

QtObject {
    property bool darkMode: FDF_DARK_MODE
    property color accentColor: FDF_ACCENT_COLOR
    property string themeName: FDF_THEME_NAME
}
"#;
    let dir = build_dir.join("qml");
    std::fs::create_dir_all(&dir).ok();
    let _ = std::fs::write(dir.join("SharedConfig.qml"), config);
    let qmldir_content = std::fs::read_to_string(dir.join("qmldir")).unwrap_or_default();
    if !qmldir_content.contains("SharedConfig") {
        let _ = std::fs::write(dir.join("qmldir"),
            qmldir_content + "module SharedConfig\nsingleton SharedConfig 1.0 SharedConfig.qml\n");
    }
}

fn write_android_project(
    build_dir: &Path,
    app_name: &str,
    package: &str,
    shared_user_id: &str,
    min_sdk: u32,
    target_sdk: u32,
) -> Result<(), String> {
    let cmake = format!(
        r#"cmake_minimum_required(VERSION 3.16)
project({app} VERSION 1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)

find_package(Qt6 REQUIRED COMPONENTS Core Gui Qml Quick)

qt_add_executable({app}
    src/main.cpp
    src/fdf_bridge.cpp
    src/sharedsettings.h
    resources.qrc
)

target_link_libraries({app} PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Qml
    Qt6::Quick
)

target_include_directories({app} PRIVATE src)

if(ANDROID)
    set_property(TARGET {app} PROPERTY
        QT_ANDROID_PACKAGE_SOURCE_DIR "${{CMAKE_CURRENT_SOURCE_DIR}}/android"
    )
    target_sources({app} PRIVATE src/sharedsettings_android.cpp)
elseif(IOS)
    target_sources({app} PRIVATE src/sharedsettings_ios.mm)
else()
    target_sources({app} PRIVATE src/sharedsettings_fallback.cpp)
endif()

qt_finalize_executable({app})
"#,
        app = app_name,
    );
    std::fs::write(build_dir.join("CMakeLists.txt"), &cmake)
        .map_err(|e| format!("failed to write CMakeLists.txt: {}", e))?;

    let shared_settings_src = Path::new(BRIDGE_SRC).join("src");
    for f in &["sharedsettings_android.cpp", "sharedsettings_fallback.cpp"] {
        let src = shared_settings_src.join(f);
        if src.exists() {
            let _ = std::fs::copy(&src, build_dir.join("src").join(f));
        }
    }

    let android_dir = build_dir.join("android");
    std::fs::create_dir_all(android_dir.join("res/values")).ok();
    std::fs::create_dir_all(android_dir.join("res/drawable")).ok();

    let manifest = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="{package}"
    android:sharedUserId="{shared_user_id}"
    android:versionCode="1"
    android:versionName="1.0">

    <uses-sdk android:minSdkVersion="{min_sdk}" android:targetSdkVersion="{target_sdk}"/>

    <application android:label="{app_title}"
        android:name="org.qtproject.qt.android.bindings.QtApplication"
        android:allowBackup="true"
        android:extractNativeLibs="true">

        <activity android:name="org.qtproject.qt.android.bindings.QtActivity"
            android:configChanges="orientation|uiMode|screenLayout|screenSize|smallestScreenSize|layoutDirection|locale|fontScale|keyboard|keyboardHidden|navigation|mcc|mnc|density"
            android:launchMode="singleTop"
            android:screenOrientation="unspecified"
            android:exported="true">

            <intent-filter>
                <action android:name="android.intent.action.MAIN"/>
                <category android:name="android.intent.category.LAUNCHER"/>
            </intent-filter>

            <meta-data android:name="android.app.lib_name" android:value="{app}"/>
        </activity>
    </application>
</manifest>
"#,
        package = package,
        shared_user_id = shared_user_id,
        min_sdk = min_sdk,
        target_sdk = target_sdk,
        app_title = app_name.replace('-', " "),
        app = app_name,
    );
    std::fs::write(android_dir.join("AndroidManifest.xml"), &manifest)
        .map_err(|e| format!("failed to write AndroidManifest.xml: {}", e))?;

    let strings_xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<resources>
    <string name="app_name">{app_title}</string>
</resources>
"#,
        app_title = app_name.replace('-', " ")
    );
    std::fs::write(android_dir.join("res/values/strings.xml"), &strings_xml).ok();

    Ok(())
}

fn write_ios_project(
    build_dir: &Path,
    app_name: &str,
    bundle_id: &str,
    deployment: &str,
) -> Result<(), String> {
    let cmake = format!(
        r#"cmake_minimum_required(VERSION 3.16)
project({app} VERSION 1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)

find_package(Qt6 REQUIRED COMPONENTS Core Gui Qml Quick)

qt_add_executable({app}
    src/main.cpp
    src/fdf_bridge.cpp
    src/sharedsettings.h
    resources.qrc
)

target_link_libraries({app} PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Qml
    Qt6::Quick
)

target_include_directories({app} PRIVATE src)

if(ANDROID)
    target_sources({app} PRIVATE src/sharedsettings_android.cpp)
elseif(IOS)
    target_sources({app} PRIVATE src/sharedsettings_ios.mm)
else()
    target_sources({app} PRIVATE src/sharedsettings_fallback.cpp)
endif()

if(IOS)
    set_target_properties({app} PROPERTIES
        MACOSX_BUNDLE_GUI_IDENTIFIER "{bundle}"
        MACOSX_BUNDLE_BUNDLE_VERSION "1.0"
        MACOSX_BUNDLE_SHORT_VERSION_STRING "1.0"
        MACOSX_BUNDLE TRUE
        XCODE_ATTRIBUTE_IPHONEOS_DEPLOYMENT_TARGET "{deployment}"
        XCODE_ATTRIBUTE_ENABLE_BITCODE "NO"
    )
endif()

qt_finalize_executable({app})
"#,
        app = app_name,
        bundle = bundle_id,
        deployment = deployment,
    );
    std::fs::write(build_dir.join("CMakeLists.txt"), &cmake)
        .map_err(|e| format!("failed to write CMakeLists.txt: {}", e))?;

    let shared_settings_src = Path::new(BRIDGE_SRC).join("src");
    for f in &["sharedsettings_ios.mm", "sharedsettings_fallback.cpp"] {
        let src = shared_settings_src.join(f);
        if src.exists() {
            let _ = std::fs::copy(&src, build_dir.join("src").join(f));
        }
    }

    let plist = generate_info_plist(app_name, bundle_id);
    std::fs::write(build_dir.join("Info.plist"), &plist)
        .map_err(|e| format!("failed to write Info.plist: {}", e))?;

    Ok(())
}

fn write_build_script(build_dir: &Path, app_name: &str, target: &str) {
    let script = match target {
        "wasm" => format!(
            r#"#!/usr/bin/env bash
set -euo pipefail
BUILD_DIR="$(cd "$(dirname "$0")" && pwd)"
echo "Building {app} for WebAssembly"
emcmake cmake -B "$BUILD_DIR" -G Ninja "$BUILD_DIR"
cmake --build "$BUILD_DIR"
echo "Build complete. Output: {app}.html, {app}.wasm, {app}.js"
"#,
            app = app_name
        ),
        "android" => format!(
            r#"#!/usr/bin/env bash
set -euo pipefail
BUILD_DIR="$(cd "$(dirname "$0")" && pwd)"
echo "Building {app} for Android"
cmake -B "$BUILD_DIR" -G Ninja \
    -DCMAKE_TOOLCHAIN_FILE="${{CMAKE_TOOLCHAIN_FILE:?}}" \
    -DANDROID_ABI=arm64-v8a \
    "$BUILD_DIR"
cmake --build "$BUILD_DIR"
"#,
            app = app_name
        ),
        "ios" => format!(
            r#"#!/usr/bin/env bash
set -euo pipefail
BUILD_DIR="$(cd "$(dirname "$0")" && pwd)"
echo "Building {app} for iOS"
cmake -B "$BUILD_DIR" -G Xcode \
    -DCMAKE_SYSTEM_NAME=iOS \
    -DCMAKE_OSX_ARCHITECTURES=arm64 \
    "$BUILD_DIR"
xcodebuild -target {app} -configuration Release
echo "Build complete"
"#,
            app = app_name
        ),
        "windows" => format!(
            r#"@echo off
setlocal
set BUILD_DIR=%~dp0
echo Building %~n0 for Windows
cmake -B "%BUILD_DIR%" -G "Ninja" "%BUILD_DIR%"
cmake --build "%BUILD_DIR%"
if errorlevel 1 exit /b 1
echo Running windeployqt...
windeployqt --release "%BUILD_DIR%\%~n0.exe"
echo Build complete
"#,
        ),
        _ => return,
    };
    let _ = std::fs::write(build_dir.join("build.sh"), &script);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            build_dir.join("build.sh"),
            std::fs::Permissions::from_mode(0o755),
        );
    }
}

fn generate_info_plist(app_name: &str, bundle_id: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>{app}</string>
    <key>CFBundleExecutable</key>
    <string>{app}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>{app}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSRequiresIPhoneOS</key>
    <true/>
    <key>UILaunchStoryboardName</key>
    <string>LaunchScreen</string>
    <key>UIRequiredDeviceCapabilities</key>
    <array>
        <string>arm64</string>
    </array>
    <key>UISupportedInterfaceOrientations</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
</dict>
</plist>
"#,
        app = app_name,
        bundle = bundle_id,
    )
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let path = entry.path();
            let dest = dst.join(entry.file_name());
            if path.is_dir() {
                std::fs::create_dir_all(&dest).ok();
                copy_dir_recursive(&path, &dest);
            } else {
                std::fs::copy(&path, &dest).ok();
            }
        }
    }
}
