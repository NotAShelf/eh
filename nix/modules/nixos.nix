self: {
  config,
  pkgs,
  lib,
  ...
}: let
  inherit (lib.modules) mkIf;
  inherit (lib.options) mkEnableOption mkPackageOption literalExpression;

  cfg = config.programs.eh;
in {
  options.programs.eh = {
    enable = mkEnableOption "eh - Ergonomic Nix CLI helper";
    package = mkPackageOption self.packages.${pkgs.stdenv.hostPlatform.system} ["eh"] {
      pkgsText = literalExpression "self.packages.$${pkgs.stdenv.hostPlatform.system}";
    };

    comma.enable = mkEnableOption "the comma-compatible frontend (using the SPAM index)";
  };

  config = mkIf cfg.enable {
    environment.systemPackages = [
      (if cfg.comma.enable then cfg.package.override {withComma = true;} else cfg.package)
    ];
  };
}
