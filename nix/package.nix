{
  lib,
  rustPlatform,
  stdenv,
}:
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "eh";
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).workspace.package.version;

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
  env.CARGO_BUILD_TARGET = stdenv.hostPlatform.rust.cargoShortTarget;
  cargoLock.lockFile = "${finalAttrs.src}/Cargo.lock";
  enableParallelBuilding = true;

  postInstall = ''
    # Install required files with the 'dist' task
    $out/bin/xtask multicall \
      --bin-dir $out/bin \
      --main-binary $out/bin/eh

    # The xtask output has been built as a part of the build phase. If
    # we don't remove it, it'll be linked in $out/bin alongside the actual
    # binary and populate $PATH with a dedicated 'xtask' command. Remove.
    rm -rf $out/bin/xtask
  '';

  meta = {
    description = "Ergonomic Nix CLI helper";
    maintainers = with lib.licenses; [NotAShelf];
    license = lib.licenses.mpl20;
    mainProgram = "eh";
  };
})
