{
  lib,
  rustPlatform,
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

  cargoLock.lockFile = "${finalAttrs.src}/Cargo.lock";
  enableParallelBuilding = true;

  meta = {
    description = "Ergonomic Nix CLI helper";
    maintainers = with lib.licenses; [NotAShelf];
  };
})
