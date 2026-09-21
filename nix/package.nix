{
  lib,
  rustPlatform,
  stdenv,
  installShellFiles,
  versionCheckHook,
}: let
  cargoManifest = lib.importTOML ../Cargo.toml;
in
  rustPlatform.buildRustPackage (finalAttrs: {
    pname = "eh";
    version = cargoManifest.workspace.package.version;
    __structuredAttrs = true;

    src = let
      fs = lib.fileset;
      s = ../.;
    in
      fs.toSource {
        root = s;
        fileset = fs.unions (map (dir: (s + /${dir})) [
          ".cargo"
          "crates"
          "eh"
          "Cargo.toml"
          "Cargo.lock"
        ]);
      };

    cargoLock.lockFile = "${finalAttrs.src}/Cargo.lock";
    enableParallelBuilding = true;

    # xtask doesn't support passing --target
    # but nix hooks expect the folder structure from when it's set
    env.CARGO_BUILD_TARGET = stdenv.hostPlatform.rust.cargoShortTarget;

    nativeInstallCheckInputs = [versionCheckHook];
    versionCheckProgram = "${placeholder "out"}/bin/${finalAttrs.meta.mainProgram}";
    versionCheckProgramArg = "--version";
    doInstallCheck = true;

    strictDeps = true;
    nativeBuildInputs = [installShellFiles];

    postInstall = ''
      # Install required files with the 'dist' task
      cargo xtask multicall \
        --bin-dir $out/bin \
        --main-binary $out/bin/eh

      # Generate shell completions and install them.
      for shell in bash zsh fish; do
        cargo xtask completions $shell
      done

      installShellCompletion completions/*
    '';

    meta = {
      description = "Ergonomic Nix CLI helper";
      homepage = "https://github.com/notashelf/eh";
      maintainers = with lib.maintainers; [NotAShelf];
      license = lib.licenses.mpl20;
      mainProgram = "eh";
    };
  })
