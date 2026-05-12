{
  mkShell,
  rustc,
  cargo,
  rustfmt,
  clippy,
  taplo,
  rust-analyzer,
  cargo-nextest,
}:
mkShell {
  name = "rust";

  strictDeps = true;
  nativeBuildInputs = [
    rustc
    cargo

    (rustfmt.override {asNightly = true;})
    clippy
    cargo
    taplo
    rust-analyzer

    cargo-nextest
  ];
}
