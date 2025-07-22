{
  mkShell,
  rust-analyzer,
  rustfmt,
  clippy,
  cargo,
  taplo,
  rustPlatform,
}:
mkShell {
  name = "rust";
  packages = [
    rust-analyzer
    rustfmt
    clippy
    cargo

    taplo
  ];

  RUST_SRC_PATH = "${rustPlatform.rustLibSrc}";
}
