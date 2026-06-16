use std::path::{Path, PathBuf};
use std::process::Command;

const APP_BRIDGE_SRC: &str = env!("CARGO_MANIFEST_DIR");

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: bridge <command> [options]");
        eprintln!();
        eprintln!("Commands:");
        eprintln!("  build       Compile qmX + IPC spec into a standalone executable");
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
    eprintln!("Usage: bridge build --input <file.qmx> --output <dir> [--fdf <dir>] [--stdlib <dir>] [--ffi <file>]");
    std::process::exit(1);
}

fn cmd_build(args: &[String]) {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut fdf_dir: Option<PathBuf> = None;
    let mut stdlib_dir: Option<PathBuf> = None;
    let mut ffi_file: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--input" | "-i" => { i += 1; input = Some(PathBuf::from(&args[i])); }
            "--output" | "-o" => { i += 1; output = Some(PathBuf::from(&args[i])); }
            "--fdf" => { i += 1; fdf_dir = Some(PathBuf::from(&args[i])); }
            "--stdlib" => { i += 1; stdlib_dir = Some(PathBuf::from(&args[i])); }
            "--ffi" => { i += 1; ffi_file = Some(PathBuf::from(&args[i])); }
            _ => { eprintln!("Unknown option: {}", args[i]); usage_build(); }
        }
        i += 1;
    }

    let input = input.unwrap_or_else(|| { eprintln!("Missing --input"); usage_build(); std::process::exit(1); });
    let output = output.unwrap_or_else(|| { eprintln!("Missing --output"); usage_build(); std::process::exit(1); });

    let base_dir = input.parent().unwrap_or(Path::new("."));
    let fdf_src = fdf_dir.unwrap_or_else(|| base_dir.join("FDF"));
    let stdlib_src = stdlib_dir.unwrap_or_else(|| base_dir.join("stdlib"));

    std::fs::create_dir_all(&output).unwrap_or_else(|e| {
        eprintln!("Failed to create output directory {}: {}", output.display(), e);
        std::process::exit(1);
    });

    let app_name = input.file_stem().unwrap_or_default().to_string_lossy();

    println!("Transpiling {}...", input.display());
    let source = std::fs::read_to_string(&input).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", input.display(), e);
        std::process::exit(1);
    });
    let base = input.parent().unwrap_or(Path::new(".")).to_string_lossy().to_string();
    let transpiled = fdf_bridge::transpile::transpile(&source, &base).unwrap_or_else(|e| {
        eprintln!("Transpilation failed: {}", e);
        std::process::exit(1);
    });

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

    let ffi_defs = if let Some(ffi) = &ffi_file {
        if ffi.exists() {
            let content = std::fs::read_to_string(ffi).unwrap_or_default();
            serde_json::from_str::<FfiDefinition>(&content).ok()
        } else { None }
    } else { None };

    let binary = compile_app(&output, &app_name, ffi_defs.as_ref(), &transpiled);
    match binary {
        Ok(path) => println!("  Built {}", path.display()),
        Err(e) => eprintln!("  Compilation skipped: {} (the transpiled QML and assets are ready, build manually with `nix build` or `cargo build`)", e),
    }

    println!("Build complete. Output: {}", output.display());
}

#[derive(serde::Deserialize)]
struct FfiDefinition {
    fns: Vec<FfiFn>,
}

#[derive(serde::Deserialize)]
struct FfiFn {
    name: String,
    description: Option<String>,
    code: String,
}

fn compile_app(output: &Path, app_name: &str, ffi_defs: Option<&FfiDefinition>, _transpiled_qml: &str) -> Result<PathBuf, String> {
    let tmp_dir = std::env::temp_dir().join(format!("fdf-build-{}", app_name));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(tmp_dir.join("src")).map_err(|e| format!("failed to create temp dir: {}", e))?;

    let cargo_toml = format!(r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
fdf-bridge = {{ path = "{}", features = ["qt"] }}
"#, app_name, APP_BRIDGE_SRC);
    std::fs::write(tmp_dir.join("Cargo.toml"), &cargo_toml).map_err(|e| format!("failed to write Cargo.toml: {}", e))?;

    let mut main_rs = String::from("fn main() {\n");
    if let Some(defs) = ffi_defs {
        for f in &defs.fns {
            let fn_name = &f.name;
            main_rs.push_str(&format!("    fdf_bridge::ffi::register(\"{}\", Box::new(|args| -> String {{\n", fn_name.replace('\\', "\\\\").replace('"', "\\\"")));
            for line in f.code.lines() {
                main_rs.push_str(&format!("        {}\n", line));
            }
            main_rs.push_str("        \"ok\".to_string()\n");
            main_rs.push_str("    }));\n");
        }
    }
    main_rs.push_str("    fdf_bridge::run_app();\n");
    main_rs.push_str("}\n");
    std::fs::write(tmp_dir.join("src/main.rs"), &main_rs).map_err(|e| format!("failed to write main.rs: {}", e))?;

    let status = Command::new("cargo")
        .args(["generate-lockfile"])
        .current_dir(&tmp_dir)
        .status()
        .map_err(|e| format!("cargo not available: {}", e))?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err("cargo generate-lockfile failed".to_string());
    }

    println!("  Compiling {}...", app_name);
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&tmp_dir)
        .status()
        .map_err(|e| format!("cargo not available: {}", e))?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err("compilation failed".to_string());
    }

    let binary = tmp_dir.join("target/release").join(app_name);
    if !binary.exists() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err("binary not found after build".to_string());
    }
    let dest = output.join(app_name);
    std::fs::copy(&binary, &dest).map_err(|e| format!("failed to copy binary: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(dest)
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
  description = "{} - FDF Application";
  inputs = {{
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    flake-utils.url = "github:numtide/flake-utils";
    fdflib.url = "github:FunctionalDesktopFoundation/fdflib";
    stdlib.url = "github:FunctionalDesktopFoundation/stdlib";
    bridge.url = "path:../../bridge";
  }};
  outputs = {{ self, nixpkgs, flake-utils, fdflib, stdlib, bridge, ... }}:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${{system}};
        fdflibPkg = fdflib.packages.${{system}}.fdflib;
        stdlibPkg = stdlib.packages.${{system}}.stdlib;
      in {{
        packages.default = pkgs.rustPlatform.buildRustPackage {{
          pname = "{}";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = with pkgs; [
            pkg-config qt6.qtbase qt6.wrapQtAppsHook makeWrapper
          ];

          buildInputs = with pkgs.qt6; [
            qtbase qtdeclarative qt5compat qtwayland
          ];

          doCheck = false;

          postInstall = ''
            mkdir -p $out/share/{}/FDF $out/share/{}/stdlib
            cp shell.qml $out/share/{}/shell.qml
            cp -r ${{fdflibPkg}}/FDF/*.qml $out/share/{}/FDF/
            cp ${{fdflibPkg}}/FDF/qmldir $out/share/{}/FDF/
            cp -r ${{stdlibPkg}}/stdlib/*.qml $out/share/{}/stdlib/
            cp ${{stdlibPkg}}/stdlib/qmldir $out/share/{}/stdlib/

            wrapProgram $out/bin/{} \
              --set FDF_QML "$out/share/{}/shell.qml" \
              --prefix QML2_IMPORT_PATH : "$out/share/{}"
          '';
        }};
      }});
}}"#, app_name, app_name, app_name, app_name, app_name, app_name, app_name, app_name, app_name, app_name);
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

    let qmx_content = format!(r#"import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import FDF
import stdlib

<AppWindow id="root"
    appTitle="{}"
    defaultWidth={{600}} defaultHeight={{480}}
    pages={{
        {{title: "Home", component: homePage}}
    }}>

    <Component id="homePage">
        <ContentView><ScrollView clip>
            <ColumnLayout spacing={{Theme.spMd}} width={{parent.width}}>
                <Item implicitHeight={{Theme.sp2xl}} />
                <Label text="{{"Welcome to {}!"}}"
                    fontPixelSize={{Theme.fs2xl}} color={{Theme.palette.textBright}} bold />
                <Label text="Edit shell.qmx to customize this application."
                    fontPixelSize={{Theme.fsSm}} color={{Theme.palette.textDim}} />
                <Item implicitHeight={{Theme.sp2xl}} />
            </ColumnLayout>
        </ScrollView></ContentView>
    </Component>

    <PageView id="pageView" anchorsFill="parent" pages={{root.pages}} />
</AppWindow>"#, app_name, app_name);
    std::fs::write(out_dir.join("shell.qmx"), &qmx_content).ok();
    println!("  Wrote shell.qmx");

    let _ = Command::new("cargo")
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
    println!("  2. Edit shell.qmx to build your UI");
    println!("  3. Edit ffi.json to add external FFI functions");
    println!("  4. nix build  (or: bridge build --input shell.qmx --output build)");
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

    // Test mode
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
