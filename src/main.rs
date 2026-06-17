use std::path::{Path, PathBuf};

mod config;
mod build_project;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: bridge <command> [options]");
        eprintln!();
        eprintln!("Commands:");
    eprintln!("  build       Compile qmX + IPC spec into a standalone executable");
    eprintln!("  build-wasm  Build with emcmake for WebAssembly");
    eprintln!("  build-windows  Build with windeployqt for Windows");
    eprintln!("  bootstrap   Scaffold a new FDF application with Nix flake");
    eprintln!("  run         Run the FDF app");
    eprintln!("  ffi         List or test external FFI definitions");
        std::process::exit(1);
    }

    match args[1].as_str() {
        "build" => cmd_build(&args[2..]),
        "bootstrap" => cmd_bootstrap(&args[2..]),
        "ffi" => cmd_ffi(&args[2..]),
        "run" => cmd_run(),
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            std::process::exit(1);
        }
    }
}

fn usage_build() {
    eprintln!("Usage: bridge build --input <file.qmx> --output <assets-dir>");
    eprintln!("       [--fdf <dir>] [--stdlib <dir>] [--ffi <file>]");
    eprintln!("       [--platform desktop|windows|android|ios|wasm] [--config <fdf.json>]");
    eprintln!("       [--window-controls true|false] [--build-dir <dir>]");
    eprintln!();
    eprintln!("  Per-platform build artifacts are placed in subdirectories:");
    eprintln!("    <assets-dir>/build-desktop   (Linux/macOS binary)");
    eprintln!("    <assets-dir>/build-windows   (Windows binary + windeployqt)");
    eprintln!("    <assets-dir>/build-android   (Android CMake project)");
    eprintln!("    <assets-dir>/build-ios       (iOS CMake/Xcode project)");
    eprintln!("    <assets-dir>/build-wasm      (WebAssembly CMake project)");
    eprintln!();
    eprintln!("  IPC (Unix Domain Sockets) is enabled only on desktop platforms (Linux/macOS/Windows).");
    eprintln!("  Window controls are enabled only on desktop platforms.");
    std::process::exit(1);
}

fn cmd_build(args: &[String]) {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut fdf_dir: Option<PathBuf> = None;
    let mut stdlib_dir: Option<PathBuf> = None;
    let mut ffi_file: Option<PathBuf> = None;
    let mut config_file: Option<PathBuf> = None;
    let mut platform_str: Option<String> = None;
    let mut window_controls: Option<bool> = None;
    let mut build_dir_override: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--input" | "-i" => { i += 1; input = Some(PathBuf::from(&args[i])); }
            "--output" | "-o" => { i += 1; output = Some(PathBuf::from(&args[i])); }
            "--fdf" => { i += 1; fdf_dir = Some(PathBuf::from(&args[i])); }
            "--stdlib" => { i += 1; stdlib_dir = Some(PathBuf::from(&args[i])); }
            "--ffi" => { i += 1; ffi_file = Some(PathBuf::from(&args[i])); }
            "--config" => { i += 1; config_file = Some(PathBuf::from(&args[i])); }
            "--platform" => { i += 1; platform_str = Some(args[i].clone()); }
            "--window-controls" => { i += 1; window_controls = Some(args[i] == "true"); }
            "--build-dir" => { i += 1; build_dir_override = Some(PathBuf::from(&args[i])); }
            _ => { eprintln!("Unknown option: {}", args[i]); usage_build(); }
        }
        i += 1;
    }

    let input = input.unwrap_or_else(|| { eprintln!("Missing --input"); usage_build(); std::process::exit(1); });
    let output = output.unwrap_or_else(|| { eprintln!("Missing --output"); usage_build(); std::process::exit(1); });

    let base_dir = input.parent().unwrap_or(Path::new("."));
    let fdf_src = fdf_dir.unwrap_or_else(|| base_dir.join("FDF"));
    let stdlib_src = stdlib_dir.unwrap_or_else(|| base_dir.join("stdlib"));

    let app_name = input.file_stem().unwrap_or_default().to_string_lossy().to_string();

    let fdf_config = config_file.as_ref().and_then(|p| {
        if p.exists() {
            match config::FdfConfig::from_file(p) {
                Ok(cfg) => {
                    println!("  Loaded config from {}", p.display());
                    Some(cfg)
                }
                Err(e) => {
                    eprintln!("  Warning: could not parse {}: {}", p.display(), e);
                    None
                }
            }
        } else {
            None
        }
    });

    let platform = platform_str.as_deref().and_then(build_project::Platform::from_str)
        .or_else(|| fdf_config.as_ref().and_then(|c| build_project::Platform::from_str(&c.target)))
        .unwrap_or(build_project::Platform::Desktop);

    let features = fdf_config.as_ref()
        .map(|c| {
            let mut f = c.features_or_default();
            if let Some(wc) = window_controls {
                f.window_controls = wc;
            }
            if platform != build_project::Platform::Desktop {
                f.window_controls = false;
            }
            f
        })
        .unwrap_or_else(|| {
            let wc = window_controls.unwrap_or(platform == build_project::Platform::Desktop);
            config::Features { window_controls: wc, ipc: true, ffi: true }
        });

    if !features.window_controls {
        println!("  Window controls disabled for {} target", platform.as_str());
    }

    let hooks_file = base_dir.join("hooks.toml");
    let hooks_config = if hooks_file.exists() {
        match fdf_bridge::hooks::HooksConfig::from_file(&hooks_file) {
            Ok(cfg) => {
                let count = cfg.hooks.len();
                if count > 0 {
                    println!("  Loaded {} hook(s) from {}", count, hooks_file.display());
                }
                let active_count = cfg.active_on_platform(platform.as_str()).len();
                if active_count == 0 {
                    println!("  No hooks active for {} platform", platform.as_str());
                }
                Some(cfg)
            }
            Err(e) => {
                eprintln!("  Warning: could not parse {}: {}", hooks_file.display(), e);
                None
            }
        }
    } else {
        if base_dir.join("hooks.toml").exists() == false {
        }
        None
    };

    std::fs::create_dir_all(&output).unwrap_or_else(|e| {
        eprintln!("Failed to create output directory {}: {}", output.display(), e);
        std::process::exit(1);
    });

    println!("Transpiling {}...", input.display());
    let source = std::fs::read_to_string(&input).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", input.display(), e);
        std::process::exit(1);
    });
    let base = input.parent().unwrap_or(Path::new(".")).to_string_lossy().to_string();
    let mut transpiled = fdf_bridge::transpile::transpile(&source, &base).unwrap_or_else(|e| {
        eprintln!("Transpilation failed: {}", e);
        std::process::exit(1);
    });

    if !features.window_controls {
        transpiled = disable_window_controls(&transpiled);
    }

    let qml_output = output.join("shell.qml");
    std::fs::write(&qml_output, &transpiled).unwrap_or_else(|e| {
        eprintln!("Failed to write {}: {}", qml_output.display(), e);
        std::process::exit(1);
    });
    println!("  Wrote {}", qml_output.display());

    let copy_dir = |src: &Path, dst: &Path, name: &str| {
        if src.exists() {
            let dst_dir = output.join(name);
            std::fs::create_dir_all(&dst_dir).ok();
            if let Ok(entries) = std::fs::read_dir(src) {
                for entry in entries.flatten() {
                    let ft = entry.file_type().ok();
                    if ft.map_or(false, |t| t.is_file()) {
                        let dest = dst_dir.join(entry.file_name());
                        std::fs::copy(&entry.path(), &dest).ok();
                    }
                }
            }
            println!("  Copied {} to {}", src.display(), dst_dir.display());
        } else {
            eprintln!("  Warning: {} not found at {}", name, src.display());
        }
    };
    copy_dir(&fdf_src, &output.join("FDF"), "FDF");
    copy_dir(&stdlib_src, &output.join("stdlib"), "stdlib");

    if let Some(ffi) = &ffi_file {
        if ffi.exists() {
            let dest = output.join("ffi.json");
            let _ = std::fs::copy(ffi, &dest);
        }
    }

    let default_config = config::FdfConfig {
        name: Some(app_name.clone()),
        target: "desktop".to_string(),
        features: None,
        android: None,
        ios: None,
        windows: None,
        wasm: None,
    };
    let hooks_cpp = hooks_config.as_ref().map(|h| {
        h.generate_cpp_config(platform.as_str())
    });

    let build_ctx = build_project::BuildCtx {
        platform: platform.clone(),
        output: &output,
        app_name: &app_name,
        transpiled_qml: &transpiled,
        build_dir_override: build_dir_override.as_deref(),
        config: fdf_config.as_ref().unwrap_or(&default_config),
        features: features.clone(),
        hooks_cpp: hooks_cpp.as_deref(),
    };

    let result = build_project::generate_project(&build_ctx);
    match result {
        Ok(path) => println!("  Built {}", path.display()),
        Err(e) => eprintln!("  Build skipped: {} (transpiled QML and assets are ready, build manually)", e),
    }

    println!("Build complete. Output: {}", output.display());
}

fn disable_window_controls(qml: &str) -> String {
    let mut result = String::new();
    for line in qml.lines() {
        let trimmed = line.trim();
        if trimmed.contains("showTrafficLights") || trimmed.contains("showWindowControls") {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            result.push_str(&format!("{}showTrafficLights: false\n", indent));
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

#[derive(serde::Deserialize)]
pub(crate) struct FfiDefinition {
    fns: Vec<FfiFn>,
}

#[derive(serde::Deserialize)]
struct FfiFn {
    name: String,
    description: Option<String>,
    code: String,
}

fn usage_bootstrap() {
    eprintln!("Usage: bridge bootstrap --name <appname> --out <dir>");
    std::process::exit(1);
}

fn cmd_bootstrap(args: &[String]) {
    let mut name: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" | "-n" => { i += 1; name = Some(args[i].clone()); }
            "--out" | "-o" => { i += 1; out = Some(PathBuf::from(&args[i])); }
            _ => { eprintln!("Unknown option: {}", args[i]); usage_bootstrap(); }
        }
        i += 1;
    }
    let app_name = name.unwrap_or_else(|| { eprintln!("Missing --name"); usage_bootstrap(); std::process::exit(1); });
    let out_dir = out.unwrap_or_else(|| PathBuf::from(&app_name));

    std::fs::create_dir_all(&out_dir).unwrap_or_else(|e| {
        eprintln!("Failed to create {}: {}", out_dir.display(), e);
        std::process::exit(1);
    });

    let flake_content = format!(r#"{{
  description = "{} - Cross-platform FDF Application (Desktop / Android / iOS / Windows / WebAssembly)";
  inputs = {{
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fdflib.url = "github:FunctionalDesktopFoundation/fdflib";
    stdlib.url = "github:FunctionalDesktopFoundation/stdlib";
    bridge.url = "github:FunctionalDesktopFoundation/bridge";
    android-nixpkgs = {{
      url = "github:tadfisher/android-nixpkgs";
      inputs.nixpkgs.follows = "nixpkgs";
    }};
  }};
  outputs = {{ self, nixpkgs, flake-utils, fdflib, stdlib, bridge, android-nixpkgs }}:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${{system}};
        fdflibPkg = fdflib.packages.${{system}}.fdflib;
        stdlibPkg = stdlib.packages.${{system}}.stdlib;
        bridgePkg = bridge.packages.${{system}}.bridge;

        androidSdk = android-nixpkgs.sdk.${{system}} (sdkPkgs: with sdkPkgs; [
          cmdline-tools-latest
          build-tools-34-0-0
          platform-tools
          platforms-android-34
          ndk-26-1-10909125
        ]);

        androidPkgs = pkgs.pkgsCross.aarch64-multiplatform;
        iosPkgs = pkgs.pkgsCross.iphone-arm64;
        mingwPkgs = pkgs.pkgsCross.mingwW64;

        appName = "{}";
        appDir = "share/${{appName}}";

        commonBridgeBuild = platform: buildDir: outDir: ''
          mkdir -p ${{outDir}}
          bridge build \
            --input shell.qmx \
            --output ${{outDir}} \
            --fdf ${{fdflibPkg}}/FDF \
            --stdlib ${{stdlibPkg}}/stdlib \
            --config fdf.json \
            --platform ${{platform}} \
            --build-dir ${{outDir}}/${{buildDir}}
        '';
      in {{
        packages = {{
          default = self.packages.${{system}}.desktop;

          desktop = pkgs.rustPlatform.buildRustPackage {{
            pname = appName;
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = with pkgs; [
              pkg-config qt6.qtbase qt6.wrapQtAppsHook makeWrapper bridgePkg
            ];

            buildInputs = with pkgs.qt6; [
              qtbase qtdeclarative qtwayland
            ];

            doCheck = false;

            preBuild = ''
              sed -i 's|../../bridge|${{bridge}}|g' Cargo.toml
            '';

            postInstall = ''
              ${{commonBridgeBuild "desktop" "build-desktop" "$out/${{appDir}}"}}

              wrapProgram $out/bin/${{appName}} \
                --set FDF_QML "$out/${{appDir}}/shell.qml" \
                --prefix QML2_IMPORT_PATH : "$out/${{appDir}}" \
                --prefix PATH : ${{pkgs.lib.makeBinPath [ bridgePkg ]}}
            '';
          }};

          android = pkgs.stdenv.mkDerivation {{
            name = "${{appName}}-android";
            src = ./.;

            nativeBuildInputs = [
              pkgs.cmake pkgs.ninja pkgs.jdk17 androidSdk bridgePkg
            ];

            buildInputs = [
              androidPkgs.qt6.qtbase
              androidPkgs.qt6.qtdeclarative
            ];

            configurePhase = "true";

            buildPhase = ''
              export ANDROID_HOME="${{androidSdk}}/share/android-sdk"
              export ANDROID_NDK_ROOT="${{androidSdk}}/share/android-sdk/ndk/26.1.10909125"
              export JAVA_HOME="${{pkgs.jdk17.home}}"
              export QT_HOST_PATH="${{pkgs.qt6.qtbase.dev}}"
              export QT_ANDROID_QT_ROOT="${{androidPkgs.qt6.qtbase}}"
              export CMAKE_TOOLCHAIN_FILE="$ANDROID_NDK_ROOT/build/cmake/android.toolchain.cmake"
              export ANDROID_ABI=arm64-v8a
              export ANDROID_PLATFORM=android-21

              ${{commonBridgeBuild "android" "build-android" "build"}}

              cd build/build-android

              cmake -B . -G Ninja \
                -DCMAKE_TOOLCHAIN_FILE="$CMAKE_TOOLCHAIN_FILE" \
                -DANDROID_ABI=$ANDROID_ABI \
                -DANDROID_PLATFORM=$ANDROID_PLATFORM \
                -DQT_HOST_PATH="$QT_HOST_PATH" \
                -DQT_HOST_PATH_CMAKE_DIR="$QT_HOST_PATH/lib/cmake" \
                -DCMAKE_FIND_ROOT_PATH="${{androidPkgs.qt6.qtbase}};${{androidPkgs.qt6.qtdeclarative}}" \
                -DQT_ANDROID_QT_ROOT="$QT_ANDROID_QT_ROOT"

              cmake --build .
            '';

            installPhase = ''
              mkdir -p $out/lib $out/apk
              find build/build-android -name "*.so" -exec cp {{}} $out/lib/ \;
              echo "Android native library built" > $out/lib/.built
              echo "To produce an APK:" > $out/apk/README.txt
              echo "  androiddeployqt --input build/build-android/android-${{appName}}-deployment-settings.json --output $out/apk" >> $out/apk/README.txt
            '';
          }};

          windows = pkgs.stdenv.mkDerivation {{
            name = "${{appName}}-windows";
            src = ./.;

            nativeBuildInputs = with pkgs; [
              bridgePkg cmake ninja
            ];

            buildInputs = with mingwPkgs; [
              qt6.qtbase qt6.qtdeclarative
            ];

            configurePhase = "true";

            buildPhase = ''
              ${{commonBridgeBuild "windows" "build-windows" "build"}}

              cd build/build-windows

              cmake -B . -G Ninja \
                -DCMAKE_SYSTEM_NAME=Windows \
                -DCMAKE_C_COMPILER=${{mingwPkgs.stdenv.cc}}/bin/${{mingwPkgs.stdenv.cc.targetPrefix}}gcc \
                -DCMAKE_CXX_COMPILER=${{mingwPkgs.stdenv.cc}}/bin/${{mingwPkgs.stdenv.cc.targetPrefix}}g++ \
                -DCMAKE_FIND_ROOT_PATH=${{mingwPkgs.qt6.qtbase}} \
                -DCMAKE_FIND_ROOT_PATH_MODE_PROGRAM=NEVER \
                -DCMAKE_FIND_ROOT_PATH_MODE_LIBRARY=ONLY \
                -DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=ONLY

              cmake --build .
            '';

            installPhase = ''
              mkdir -p $out/bin
              find build/build-windows -name "*.exe" -exec cp {{}} $out/bin/ \;
              echo "Windows build: $out/bin/${{appName}}.exe"
            '';
          }};

          ios = pkgs.stdenv.mkDerivation {{
            name = "${{appName}}-ios";
            src = ./.;

            nativeBuildInputs = [ pkgs.cmake pkgs.ninja bridgePkg ]
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.xcbuild ];

            buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
              iosPkgs.qt6.qtbase
              iosPkgs.qt6.qtdeclarative
            ];

            configurePhase = "true";

            buildPhase = ''
              ${{commonBridgeBuild "ios" "build-ios" "build"}}

              cd build/build-ios

              cmake -B . -G Xcode \
                -DCMAKE_SYSTEM_NAME=iOS \
                -DCMAKE_OSX_ARCHITECTURES=arm64 \
                -DCMAKE_OSX_DEPLOYMENT_TARGET=15.0

              xcodebuild -target ${{appName}} -configuration Release \
                DEVELOPMENT_TEAM="" \
                CODE_SIGN_IDENTITY="" \
                CODE_SIGNING_REQUIRED=NO
            '';

            installPhase = ''
              mkdir -p $out/bin
              find build/build-ios -name "*.app" -type d -exec cp -R {{}} $out/bin/ \;
              echo "iOS build: $out/bin/" >> $out/README.txt
            '';
          }};

          wasm = pkgs.stdenv.mkDerivation {{
            name = "${{appName}}-wasm";
            src = ./.;

            nativeBuildInputs = with pkgs; [
              bridgePkg cmake ninja emscripten
            ];

            configurePhase = "true";

            buildPhase = ''
              source ${{pkgs.emscripten}}/share/emscripten/emsdk_env.sh 2>/dev/null || true

              ${{commonBridgeBuild "wasm" "build-wasm" "build"}}

              cd build/build-wasm

              emcmake cmake -B . -G Ninja \
                -DCMAKE_BUILD_TYPE=Release \
                -DCMAKE_FIND_ROOT_PATH="${{pkgs.qt6.qtbase}}" \
                -DQT_HOST_PATH="${{pkgs.qt6.qtbase.dev}}"

              cmake --build .
            '';

            installPhase = ''
              mkdir -p $out/bin
              find build/build-wasm -name "*.html" -exec cp {{}} $out/bin/ \;
              find build/build-wasm -name "*.wasm" -exec cp {{}} $out/bin/ \;
              find build/build-wasm -name "*.js" -exec cp {{}} $out/bin/ \;
              echo "WebAssembly build: $out/bin/" >> $out/README.txt
            '';
          }};
        }};

        devShells = {{
          default = pkgs.mkShell {{
            name = "${{appName}}-dev";
            nativeBuildInputs = with pkgs; [
              cmake ninja bridgePkg qt6.wrapQtAppsHook
            ];
            buildInputs = with pkgs.qt6; [
              qtbase qtdeclarative qtwayland
            ];
            shellHook = ''
              echo "FDF ${{appName}} dev shell"
              echo "  bridge build --input shell.qmx --output build --platform desktop"
            '';
          }};

          android = pkgs.mkShell {{
            name = "${{appName}}-android-env";
            nativeBuildInputs = with pkgs; [
              cmake ninja jdk17 bridgePkg androidSdk
            ];
            buildInputs = [
              androidPkgs.qt6.qtbase
              androidPkgs.qt6.qtdeclarative
            ];
            shellHook = ''
              export ANDROID_HOME="${{androidSdk}}/share/android-sdk"
              export ANDROID_NDK_ROOT="${{androidSdk}}/share/android-sdk/ndk/26.1.10909125"
              export JAVA_HOME="${{pkgs.jdk17.home}}"
              export QT_HOST_PATH="${{pkgs.qt6.qtbase.dev}}"
              export QT_ANDROID_QT_ROOT="${{androidPkgs.qt6.qtbase}}"
              export CMAKE_TOOLCHAIN_FILE="$ANDROID_NDK_ROOT/build/cmake/android.toolchain.cmake"
              echo "  bridge build --input shell.qmx --output build --platform android"
              echo "  cd build/build-android && cmake -B . -G Ninja \\"
              echo "    -DCMAKE_TOOLCHAIN_FILE=\$CMAKE_TOOLCHAIN_FILE \\"
              echo "    -DANDROID_ABI=arm64-v8a \\"
              echo "    -DANDROID_PLATFORM=android-21 \\"
              echo "    -DQT_HOST_PATH=\$QT_HOST_PATH \\"
              echo "    -DQT_ANDROID_QT_ROOT=\$QT_ANDROID_QT_ROOT"
              echo "  cmake --build ."
            '';
          }};

          windows = pkgs.mkShell {{
            name = "${{appName}}-windows-env";
            nativeBuildInputs = with pkgs; [
              cmake ninja bridgePkg mingwPkgs.stdenv.cc
            ];
            buildInputs = with mingwPkgs; [
              qt6.qtbase qt6.qtdeclarative
            ];
            shellHook = ''
              echo "Windows cross dev shell"
              echo "  bridge build --input shell.qmx --output build --platform windows"
              echo "  cd build/build-windows && cmake -B . -G Ninja \\"
              echo "    -DCMAKE_SYSTEM_NAME=Windows \\"
              echo "    -DCMAKE_C_COMPILER=${{mingwPkgs.stdenv.cc.targetPrefix}}gcc \\"
              echo "    -DCMAKE_CXX_COMPILER=${{mingwPkgs.stdenv.cc.targetPrefix}}g++"
              echo "  cmake --build ."
            '';
          }};

          wasm = pkgs.mkShell {{
            name = "${{appName}}-wasm-env";
            nativeBuildInputs = with pkgs; [
              cmake ninja bridgePkg emscripten
            ];
            buildInputs = with pkgs.qt6; [
              qtbase qtdeclarative
            ];
            shellHook = ''
              export EMSDK="${{pkgs.emscripten}}/share/emscripten"
              export EM_CACHE="$PWD/.emcache"
              echo "WebAssembly dev shell"
              echo "  source \$EMSDK/emsdk_env.sh"
              echo "  bridge build --input shell.qmx --output build --platform wasm"
              echo "  cd build/build-wasm && emcmake cmake -B . -G Ninja && cmake --build ."
            '';
          }};
        }} // pkgs.lib.optionalAttrs pkgs.stdenv.isDarwin {{
          ios = pkgs.mkShell {{
            name = "${{appName}}-ios-env";
            nativeBuildInputs = with pkgs; [
              cmake ninja bridgePkg
            ];
            buildInputs = [
              iosPkgs.qt6.qtbase
              iosPkgs.qt6.qtdeclarative
            ];
            shellHook = ''
              export DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer"
              export SDKROOT="/Applications/Xcode.app/Contents/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk"
              echo "iOS dev shell"
              echo "  bridge build --input shell.qmx --output build --platform ios"
              echo "  cd build/build-ios && cmake -B . -G Xcode \\"
              echo "    -DCMAKE_SYSTEM_NAME=iOS \\"
              echo "    -DCMAKE_OSX_ARCHITECTURES=arm64"
              echo "  xcodebuild -target ${{appName}} -configuration Release"
            '';
          }};
        }};
      }}
    );
}}"#, app_name, app_name);
    std::fs::write(out_dir.join("flake.nix"), &flake_content).ok();
    println!("  Wrote flake.nix");

    let cargo_content = format!(r#"[workspace]

[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
fdf-bridge = {{ path = "../../bridge", features = ["qt"] }}
"#, app_name);
    std::fs::write(out_dir.join("Cargo.toml"), &cargo_content).ok();
    println!("  Wrote Cargo.toml");

    let fdf_json = r#"{
  "name": "demo-app",
  "target": "desktop",
  "features": {
    "windowControls": true,
    "ipc": true,
    "ffi": true
  },
  "android": {
    "packageName": "org.fdf.app",
    "sharedUserId": "com.fdf.shared_settings_group",
    "minSdk": 21,
    "targetSdk": 33,
    "buildDir": "build-android"
  },
  "ios": {
    "bundleId": "org.fdf.app",
    "deploymentTarget": "15.0",
    "buildDir": "build-ios"
  },
  "windows": {
    "buildDir": "build-windows"
  },
  "wasm": {
    "buildDir": "build-wasm"
  }
}"#;
    std::fs::write(out_dir.join("fdf.json"), fdf_json).ok();
    println!("  Wrote fdf.json");

    std::fs::create_dir_all(out_dir.join("src")).ok();
    let main_content = "fn main() {\n    fdf_bridge::run_app();\n}\n";
    std::fs::write(out_dir.join("src/main.rs"), main_content).ok();
    println!("  Wrote src/main.rs");

    let ffi_content = r#"{
  "fns": [
    {
      "name": "myFunction",
      "description": "Example external FFI function",
      "code": ""
    }
  ]
}"#;
    std::fs::write(out_dir.join("ffi.json"), ffi_content).ok();
    println!("  Wrote ffi.json");

    let hooks_content = r#"# Example hooks.toml
# Define C++ service processes that interact with the QML frontend.
#
# [hooks.<name>]
# command = "executable-path-or-name"    # Required: command to run
# type = "service"                         # "service" (long-running) or "oneshot" (run once)
# autostart = true                         # Start on app launch
# platforms = ["linux", "macos"]           # Empty = all platforms
# description = "What this hook does"
# args = ["--flag", "value"]               # Default arguments
# timeout_ms = 5000                        # Response timeout for call()

[hooks.example]
command = "fdf-example-hook"
type = "service"
autostart = false
description = "Example hook service"
"#;
    std::fs::write(out_dir.join("hooks.toml"), hooks_content).ok();
    println!("  Wrote hooks.toml");

    let qmx_content = format!(r#"import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import FDF
import stdlib

<AppWindow id="root"
    appTitle="{}"
    defaultWidth={{600}} defaultHeight={{480}}
    sidebarWidth={{200}}
    showTrafficLights={{true}}
    currentPage={{0}}
    pages={{
        {{title: "Home", component: homePage, icon: "\uf015"}},
        {{title: "About", component: aboutPage, icon: "\uf05a"}}
    }}
    sidebarComponent={{sidebarComp}}>

    <Component id="sidebarComp">
        <Sidebar highlightIndex={{root.currentPage}}>
            <Repeater model={{root.pages}}>
                <SidebarItem icon={{modelData.icon || ""}}
                    text={{modelData.title}}
                    highlighted={{index === root.currentPage}}
                    onClicked={{function() {{ root.currentPage = index }} }} />
            </Repeater>
        </Sidebar>
    </Component>

    <Component id="homePage">
        <ContentView><ScrollView clip>
            <ColumnLayout spacing={{Theme.spMd}} width={{parent.width}}>
                <Item implicitHeight={{Theme.sp2xl}} />
                <Label text="{{"Welcome to {}!"}}"
                    fontPixelSize={{Theme.fs2xl}} color={{Theme.palette.textBright}} bold />
                <NumberCounter value={{0}} fontPixelSize={{Theme.fsSm}}
                    color={{Theme.palette.textDim}} />
                <Label text="Edit shell.qmx to customize this application."
                    fontPixelSize={{Theme.fsSm}} color={{Theme.palette.textDim}} />
                <Item implicitHeight={{Theme.sp2xl}} />
            </ColumnLayout>
        </ScrollView></ContentView>
    </Component>

    <Component id="aboutPage">
        <ContentView><ScrollView clip>
            <ColumnLayout spacing={{Theme.spMd}} width={{parent.width}}>
                <Item implicitHeight={{Theme.sp2xl}} />
                <Label text="About" fontPixelSize={{Theme.fs2xl}}
                    color={{Theme.palette.textBright}} bold />
                <Label text="Built with FDF Bridge + Qt6"
                    fontPixelSize={{Theme.fsSm}} color={{Theme.palette.textDim}} />
                <Item implicitHeight={{Theme.sp2xl}} />
            </ColumnLayout>
        </ScrollView></ContentView>
    </Component>

    <PageView id="pageView" anchorsFill="parent" pages={{root.pages}} />
</AppWindow>"#, app_name, app_name);
    std::fs::write(out_dir.join("shell.qmx"), &qmx_content).ok();
    println!("  Wrote shell.qmx");

    let _ = std::process::Command::new("cargo")
        .args(["generate-lockfile"])
        .current_dir(&out_dir)
        .status();

    std::fs::write(out_dir.join(".gitignore"), "result\ntarget\n").ok();
    println!("  Wrote .gitignore");

    println!();
    println!("Bootstrapped '{}' project in {}", app_name, out_dir.display());
    println!();
    println!("Next steps:");
    println!("  1. cd {}", out_dir.display());
    println!("  2. nix flake lock  (generate lockfiles)");
    println!("  3. nix build .#desktop       (Linux/macOS desktop)");
    println!("  4. nix build .#android       (Android NDK cross-build)");
    println!("  5. nix build .#windows       (MinGW cross-build)");
    println!("  6. nix build .#wasm          (WebAssembly via emscripten)");
    println!("  7. nix build .#ios           (iOS, macOS only)");
    println!("  8. nix develop .#android     (Android dev shell)");
    println!("  9. nix develop .#wasm        (WebAssembly dev shell)");
}

fn cmd_ffi(args: &[String]) {
    if args.len() < 2 {
        eprintln!("Usage: bridge ffi <file.json> [--test <fnname> <args>]");
        std::process::exit(1);
    }
    let ffi_path = PathBuf::from(&args[1]);
    if !ffi_path.exists() {
        eprintln!("FFI file not found: {}", ffi_path.display());
        std::process::exit(1);
    }

    let content = std::fs::read_to_string(&ffi_path).unwrap_or_else(|e| {
        eprintln!("Error reading: {}", e);
        std::process::exit(1);
    });

    let def: FfiDefinition = serde_json::from_str(&content).unwrap_or_else(|e| {
        eprintln!("Error parsing FFI JSON: {}", e);
        std::process::exit(1);
    });

    println!("FFI file: {}", ffi_path.display());
    for f in &def.fns {
        let desc = f.description.as_deref().unwrap_or("");
        println!("  {} {}", f.name, desc);
    }
    println!("Total: {} functions", def.fns.len());

    if args.len() >= 4 && args[2] == "--test" {
        let test_name = &args[3];
        let test_args: Vec<String> = args[4..].to_vec();
        let found = def.fns.iter().find(|f| f.name == *test_name);
        match found {
            Some(_f) => println!("Test mode: would call '{}' with {:?}", test_name, test_args),
            None => eprintln!("Function '{}' not found in FFI definitions", test_name),
        }
    }
}

fn cmd_run() {
    fdf_bridge::run_app();
}
