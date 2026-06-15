use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: bridge <command> [options]");
        eprintln!();
        eprintln!("Commands:");
        eprintln!("  build       AoT transpile qmX and produce standalone executable");
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

    let app_name = input.file_stem().unwrap_or_default().to_string_lossy();
    let qml_output = output.join(format!("{}.qml", app_name));
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
            println!("  Processing FFI definitions from {}", ffi.display());
            process_ffi_definitions(ffi, &output, &app_name);
        }
    }


    let run_script = output.join("run.sh");
    let script_content = format!(r#"#!/usr/bin/env bash
DIR="$(cd "$(dirname "$0")" && pwd)"
FDF_QML="$DIR/{app_name}.qml" exec "$DIR/fdf-app"
"#);
    std::fs::write(&run_script, script_content).ok();
    let _ = std::process::Command::new("chmod").arg("+x").arg(&run_script).status();
    println!("  Wrote {}", run_script.display());


    let bridge_lib = build_bridge(&output);
    match bridge_lib {
        Ok(path) => println!("  Built {}", path.display()),
        Err(e) => eprintln!("  Bridge build skipped: {}", e),
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

fn process_ffi_definitions(ffi_path: &Path, output: &Path, app_name: &str) {
    let content = std::fs::read_to_string(ffi_path).unwrap_or_else(|e| {
        eprintln!("  Error reading FFI file: {}", e);
        std::process::exit(1);
    });

    let def: FfiDefinition = serde_json::from_str(&content).unwrap_or_else(|e| {
        eprintln!("  Error parsing FFI JSON: {}", e);
        std::process::exit(1);
    });

    if def.fns.is_empty() {
        println!("  No FFI functions defined");
        return;
    }

    let mut rs_code = String::from("// Auto-generated FFI registrations\n");
    rs_code.push_str("use std::collections::HashMap;\n\n");
    rs_code.push_str("pub fn register_app_fns() -> HashMap<String, Box<dyn Fn(Vec<String>) -> String + Send>> {\n");
    rs_code.push_str("    let mut fns: HashMap<String, Box<dyn Fn(Vec<String>) -> String + Send>> = HashMap::new();\n");

    for (idx, f) in def.fns.iter().enumerate() {
        let fn_name = &f.name;
        let fn_id = format!("_ffi_fn_{}", idx);
        rs_code.push_str(&format!("    fns.insert(\"{}\".to_string(), Box::new(|args| -> String {{\n", fn_name));
        rs_code.push_str("        // User-defined FFI function\n");
        for line in f.code.lines() {
            rs_code.push_str(&format!("        {}\n", line));
        }
        rs_code.push_str("        \"ok\".to_string()\n");
        rs_code.push_str("    }));\n");
    }

    rs_code.push_str("    fns\n");
    rs_code.push_str("}\n");

    let rs_out = output.join("gen_ffi.rs");
    std::fs::write(&rs_out, &rs_code).ok();
    println!("  Generated {}", rs_out.display());
    println!("  Registered {} FFI functions", def.fns.len());
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
  }};
  outputs = {{ self, nixpkgs, flake-utils, fdflib, stdlib, ... }}:
    flake-utils.lib.eachDefaultSystem (system:
      let pkgs = nixpkgs.legacyPackages.system;
      in {{
        packages.default = pkgs.stdenv.mkDerivation {{
          name = "{}";
          src = ./.;
          buildInputs = with pkgs; [ qt6.qtbase qt6.qtdeclarative qt6.qt5compat ];
          installPhase = ''
            mkdir -p $out/share/{}/FDF $out/share/{}/stdlib
            cp shell.qml $out/share/{}/shell.qml
            cp -r ${{fdflib}}/FDF/* $out/share/{}/FDF/
            cp -r ${{stdlib}}/stdlib/* $out/share/{}/stdlib/
            mkdir -p $out/bin
            echo '#!/usr/bin/env bash' > $out/bin/{}
            echo 'TRASH_QML=$out/share/{}/shell.qml exec ${{fdflib}}/bin/fdf-app' >> $out/bin/{}
            chmod +x $out/bin/{}
          '';
        }};
      }});
}}"#, app_name, app_name, app_name, app_name, app_name, app_name, app_name, app_name, app_name, app_name, app_name);
    std::fs::write(out_dir.join("flake.nix"), &flake_content).ok();
    println!("  Wrote flake.nix");

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

    // .gitignore
    std::fs::write(out_dir.join(".gitignore"), "result\n").ok();
    println!("  Wrote .gitignore");

    println!();
    println!("Bootstrapped '{}' project in {}", app_name, out_dir.display());
    println!();
    println!("Next steps:");
    println!("  1. cd {}", out_dir.display());
    println!("  2. Edit shell.qmx to build your UI");
    println!("  3. Edit ffi.json to add external FFI functions");
    println!("  4. bridge build --input shell.qmx --output build");
    println!("  5. nix build  (or use the Nix flake)");
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

fn build_bridge(_output: &Path) -> Result<PathBuf, String> {
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "fdf-bridge"])
        .status()
        .map_err(|e| format!("cargo not available: {}", e))?;
    if !status.success() {
        return Err("bridge build failed".to_string());
    }
    let target_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()));
    let release_lib = target_dir.parent().unwrap_or(&target_dir).join("target").join("release").join("libfdf_bridge.rlib");
    Ok(release_lib)
}

fn cmd_run() {
    fdf_bridge::run_app();
}
