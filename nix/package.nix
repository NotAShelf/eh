{
  lib,
  rustPlatform,
  stdenv,
  installShellFiles,
  versionCheckHook,
}:
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "eh";
  version = (lib.importTOML ../Cargo.toml).workspace.package.version;

  src = let
    fs = lib.fileset;
    s = ../.;
  in
    fs.toSource {
      root = s;
      fileset = fs.unions (map (dir: (s + /${dir})) [
        ".cargo"
        "eh"
        "xtask"
        "Cargo.toml"
        "Cargo.lock"
      ]);
    };

  # xtask doesn't support passing --targe
  # but nix hooks expect the folder structure from when it's set
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
    $out/bin/xtask multicall \
      --bin-dir $out/bin \
      --main-binary $out/bin/eh

    # Generate shell completions and install them.
    for shell in bash zsh fish; do
      $out/bin/xtask completions $shell
    done

    installShellCompletion completions/*

    # The xtask output has been built as a part of the build phase. If
    # we don't remove it, it'll be linked in $out/bin alongside the actual
    # binary and populate $PATH with a dedicated 'xtask' command. Remove.
    rm -rf $out/bin/xtask
  '';

  meta = {
    description = "Ergonomic Nix CLI helper";
    maintainers = with lib.maintainers; [NotAShelf];
    license = lib.licenses.mpl20;
    mainProgram = "eh";
  };
})
