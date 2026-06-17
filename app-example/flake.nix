{
  description = "app-example - FDF Application (Desktop / Android / iOS / Windows / WebAssembly)";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fdflib.url = "github:FunctionalDesktopFoundation/fdflib";
    stdlib.url = "github:FunctionalDesktopFoundation/stdlib";
    bridge.url = "path:../";

    android-nixpkgs = {
      url = "github:tadfisher/android-nixpkgs";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs = { self, nixpkgs, flake-utils, fdflib, stdlib, bridge, android-nixpkgs }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        fdflibPkg = fdflib.packages.${system}.fdflib;
        stdlibPkg = stdlib.packages.${system}.stdlib;
        bridgePkg = bridge.packages.${system}.bridge;

        androidSdk = android-nixpkgs.sdk.${system} (sdkPkgs: with sdkPkgs; [
          cmdline-tools-latest
          build-tools-34-0-0
          platform-tools
          platforms-android-34
          ndk-26-1-10909125
        ]);

        androidPkgs = pkgs.pkgsCross.aarch64-multiplatform;
        iosPkgs = pkgs.pkgsCross.iphone-arm64;
        mingwPkgs = pkgs.pkgsCross.mingwW64;
        emscriptenPkgs = pkgs.emscripten;

        appName = "app-example";
        appDir = "share/${appName}";

        commonBridgeBuild = platform: buildDir: outDir: ''
          mkdir -p ${outDir}
          bridge build \
            --input shell.qmx \
            --output ${outDir} \
            --fdf ${fdflibPkg}/FDF \
            --stdlib ${stdlibPkg}/stdlib \
            --config fdf.json \
            --platform ${platform} \
            --build-dir ${outDir}/${buildDir}
        '';
      in {
        packages = {
          default = self.packages.${system}.desktop;

          desktop = pkgs.rustPlatform.buildRustPackage {
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
              sed -i 's|path = "../"|path = "${bridge}"|g' Cargo.toml
            '';

            postInstall = ''
              ${commonBridgeBuild "desktop" "build-desktop" "$out/${appDir}"}

              wrapProgram $out/bin/${appName} \
                --set FDF_QML "$out/${appDir}/shell.qml" \
                --prefix QML2_IMPORT_PATH : "$out/${appDir}" \
                --prefix PATH : ${pkgs.lib.makeBinPath [ bridgePkg ]}
            '';
          };

          android = pkgs.stdenv.mkDerivation {
            name = "${appName}-android";
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
              export ANDROID_HOME="${androidSdk}/share/android-sdk"
              export ANDROID_NDK_ROOT="${androidSdk}/share/android-sdk/ndk/26.1.10909125"
              export JAVA_HOME="${pkgs.jdk17.home}"
              export QT_HOST_PATH="${pkgs.qt6.qtbase.dev}"
              export QT_ANDROID_QT_ROOT="${androidPkgs.qt6.qtbase}"
              export CMAKE_TOOLCHAIN_FILE="$ANDROID_NDK_ROOT/build/cmake/android.toolchain.cmake"
              export ANDROID_ABI=arm64-v8a
              export ANDROID_PLATFORM=android-21

              ${commonBridgeBuild "android" "build-android" "build"}

              cd build/build-android

              cmake -B . -G Ninja \
                -DCMAKE_TOOLCHAIN_FILE="$CMAKE_TOOLCHAIN_FILE" \
                -DANDROID_ABI=$ANDROID_ABI \
                -DANDROID_PLATFORM=$ANDROID_PLATFORM \
                -DQT_HOST_PATH="$QT_HOST_PATH" \
                -DQT_HOST_PATH_CMAKE_DIR="$QT_HOST_PATH/lib/cmake" \
                -DCMAKE_FIND_ROOT_PATH="${androidPkgs.qt6.qtbase};${androidPkgs.qt6.qtdeclarative}" \
                -DQT_ANDROID_QT_ROOT="$QT_ANDROID_QT_ROOT"

              cmake --build .
            '';

            installPhase = ''
              mkdir -p $out/lib $out/apk
              find build/build-android -name "*.so" -exec cp {} $out/lib/ \;
              find build/build-android -name "*.apk" -exec cp {} $out/apk/ \; 2>/dev/null || true
              echo "Android build complete." > $out/apk/README.txt
              echo "Native .so: $out/lib/" >> $out/apk/README.txt
              echo "" >> $out/apk/README.txt
              echo "To produce a final APK, run androiddeployqt:" >> $out/apk/README.txt
              echo "  androiddeployqt --input build/build-android/android-${appName}-deployment-settings.json --output $out/apk" >> $out/apk/README.txt
              echo "" >> $out/apk/README.txt
              echo "Or open the project in Qt Creator for full Android deployment." >> $out/apk/README.txt
            '';
          };

          windows = pkgs.stdenv.mkDerivation {
            name = "${appName}-windows";
            src = ./.;

            nativeBuildInputs = with pkgs; [
              bridgePkg cmake ninja
              mingwPkgs.windows.pthreads
            ];

            buildInputs = with mingwPkgs; [
              qt6.qtbase qt6.qtdeclarative
            ];

            configurePhase = "true";

            buildPhase = ''
              ${commonBridgeBuild "windows" "build-windows" "build"}

              cd build/build-windows

              cmake -B . -G Ninja \
                -DCMAKE_SYSTEM_NAME=Windows \
                -DCMAKE_C_COMPILER=${mingwPkgs.stdenv.cc}/bin/${mingwPkgs.stdenv.cc.targetPrefix}gcc \
                -DCMAKE_CXX_COMPILER=${mingwPkgs.stdenv.cc}/bin/${mingwPkgs.stdenv.cc.targetPrefix}g++ \
                -DCMAKE_FIND_ROOT_PATH=${mingwPkgs.qt6.qtbase} \
                -DCMAKE_FIND_ROOT_PATH_MODE_PROGRAM=NEVER \
                -DCMAKE_FIND_ROOT_PATH_MODE_LIBRARY=ONLY \
                -DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=ONLY

              cmake --build .
            '';

            installPhase = ''
              mkdir -p $out/bin
              find build/build-windows -name "*.exe" -exec cp {} $out/bin/ \;
              ${pkgs.windeployqt}/bin/windeployqt --release $out/bin/${appName}.exe 2>/dev/null || true
              cp build/build-windows/*.dll $out/bin/ 2>/dev/null || true
              echo "Windows build: $out/bin/${appName}.exe"
            '';
          };

          ios = pkgs.stdenv.mkDerivation {
            name = "${appName}-ios";
            src = ./.;

            nativeBuildInputs = [ pkgs.cmake pkgs.ninja bridgePkg ]
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.xcbuild ];

            buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
              iosPkgs.qt6.qtbase
              iosPkgs.qt6.qtdeclarative
            ];

            configurePhase = "true";

            buildPhase = ''
              ${commonBridgeBuild "ios" "build-ios" "build"}

              cd build/build-ios

              cmake -B . -G Xcode \
                -DCMAKE_SYSTEM_NAME=iOS \
                -DCMAKE_OSX_ARCHITECTURES=arm64 \
                -DCMAKE_OSX_DEPLOYMENT_TARGET=15.0

              xcodebuild -target ${appName} -configuration Release \
                DEVELOPMENT_TEAM="" \
                CODE_SIGN_IDENTITY="" \
                CODE_SIGNING_REQUIRED=NO
            '';

            installPhase = ''
              mkdir -p $out/bin
              find build/build-ios -name "*.app" -type d -exec cp -R {} $out/bin/ \;
              echo "iOS build: $out/bin/" >> $out/README.txt
            '';
          };

          wasm = pkgs.stdenv.mkDerivation {
            name = "${appName}-wasm";
            src = ./.;

            nativeBuildInputs = with pkgs; [
              bridgePkg cmake ninja emscripten
            ];

            configurePhase = "true";

            buildPhase = ''
              source ${pkgs.emscripten}/share/emscripten/emsdk_env.sh 2>/dev/null || true

              ${commonBridgeBuild "wasm" "build-wasm" "build"}

              cd build/build-wasm

              emcmake cmake -B . -G Ninja \
                -DCMAKE_BUILD_TYPE=Release \
                -DCMAKE_FIND_ROOT_PATH="${pkgs.qt6.qtbase}" \
                -DQT_HOST_PATH="${pkgs.qt6.qtbase.dev}"

              cmake --build .
            '';

            installPhase = ''
              mkdir -p $out/bin
              find build/build-wasm -name "*.html" -exec cp {} $out/bin/ \;
              find build/build-wasm -name "*.wasm" -exec cp {} $out/bin/ \;
              find build/build-wasm -name "*.js" -exec cp {} $out/bin/ \;
              find build/build-wasm -name "*.data" -exec cp {} $out/bin/ \; 2>/dev/null || true
              echo "WebAssembly build: $out/bin/" >> $out/README.txt
            '';
          };
        };

        devShells = {
          default = pkgs.mkShell {
            name = "${appName}-dev";
            nativeBuildInputs = with pkgs; [
              cmake ninja bridgePkg
              qt6.wrapQtAppsHook
            ];
            buildInputs = with pkgs.qt6; [
              qtbase qtdeclarative qtwayland
            ];
            shellHook = ''
              echo "FDF ${appName} development shell"
              echo "  bridge build --input shell.qmx --output build --platform desktop"
              echo "  cmake -B build-desktop -G Ninja build/build-desktop && cmake --build build-desktop"
            '';
          };

          android = pkgs.mkShell {
            name = "${appName}-android-env";
            nativeBuildInputs = with pkgs; [
              cmake ninja jdk17 bridgePkg androidSdk
            ];
            buildInputs = [
              androidPkgs.qt6.qtbase
              androidPkgs.qt6.qtdeclarative
            ];
            shellHook = ''
              export ANDROID_HOME="${androidSdk}/share/android-sdk"
              export ANDROID_NDK_ROOT="${androidSdk}/share/android-sdk/ndk/26.1.10909125"
              export JAVA_HOME="${pkgs.jdk17.home}"
              export QT_HOST_PATH="${pkgs.qt6.qtbase.dev}"
              export QT_ANDROID_QT_ROOT="${androidPkgs.qt6.qtbase}"
              export CMAKE_TOOLCHAIN_FILE="$ANDROID_NDK_ROOT/build/cmake/android.toolchain.cmake"

              echo "Android dev shell ready"
              echo "  bridge build --input shell.qmx --output build --platform android"
              echo "  cd build/build-android && cmake -B . -G Ninja \\"
              echo "    -DCMAKE_TOOLCHAIN_FILE=\$CMAKE_TOOLCHAIN_FILE \\"
              echo "    -DANDROID_ABI=arm64-v8a \\"
              echo "    -DANDROID_PLATFORM=android-21 \\"
              echo "    -DQT_HOST_PATH=\$QT_HOST_PATH \\"
              echo "    -DQT_ANDROID_QT_ROOT=\$QT_ANDROID_QT_ROOT"
              echo "  cmake --build ."
            '';
          };

          windows = pkgs.mkShell {
            name = "${appName}-windows-env";
            nativeBuildInputs = with pkgs; [
              cmake ninja bridgePkg
              mingwPkgs.stdenv.cc
            ];
            buildInputs = with mingwPkgs; [
              qt6.qtbase qt6.qtdeclarative
            ];
            shellHook = ''
              export CROSS_COMPILE=1
              echo "Windows cross dev shell ready"
              echo "  bridge build --input shell.qmx --output build --platform windows"
              echo "  cd build/build-windows && cmake -B . -G Ninja \\"
              echo "    -DCMAKE_SYSTEM_NAME=Windows \\"
              echo "    -DCMAKE_C_COMPILER=${mingwPkgs.stdenv.cc.targetPrefix}gcc \\"
              echo "    -DCMAKE_CXX_COMPILER=${mingwPkgs.stdenv.cc.targetPrefix}g++"
              echo "  cmake --build ."
            '';
          };

          wasm = pkgs.mkShell {
            name = "${appName}-wasm-env";
            nativeBuildInputs = with pkgs; [
              cmake ninja bridgePkg emscripten
            ];
            buildInputs = with pkgs.qt6; [
              qtbase qtdeclarative
            ];
            shellHook = ''
              export EMSDK="${pkgs.emscripten}/share/emscripten"
              export EM_CACHE="$PWD/.emcache"
              echo "WebAssembly dev shell ready"
              echo "  source \$EMSDK/emsdk_env.sh"
              echo "  bridge build --input shell.qmx --output build --platform wasm"
              echo "  cd build/build-wasm && emcmake cmake -B . -G Ninja && cmake --build ."
            '';
          };
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isDarwin {
          ios = pkgs.mkShell {
            name = "${appName}-ios-env";
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
              echo "iOS dev shell ready"
              echo "  bridge build --input shell.qmx --output build --platform ios"
              echo "  cd build/build-ios && cmake -B . -G Xcode \\"
              echo "    -DCMAKE_SYSTEM_NAME=iOS \\"
              echo "    -DCMAKE_OSX_ARCHITECTURES=arm64"
              echo "  xcodebuild -target ${appName} -configuration Release"
            '';
          };
        };
      }
    );
}
