self: {
  config,
  pkgs,
  lib,
  ...
}: let
  inherit (lib.modules) mkIf;
  inherit (lib.options) mkEnableOption mkPackageOption;
  inherit (lib.strings) optionalString;

  cfg = config.programs.eh;
in {
  options.programs.eh = {
    enable = mkEnableOption "eh - Ergonomic Nix CLI helper";
    package = mkPackageOption self.packages.${pkgs.hostPlatform.system} ["eh"] {};

    hooks = {
      bash.enable = mkEnableOption "Bash shell hook for EH" // {default = config.programs.bash.enable;};
      zsh.enable = mkEnableOption "ZSH shell hook for EH" // {default = config.programs.zsh.enable;};
    };
  };

  config = mkIf cfg.enable {
    environment.systemPackages = [cfg.package];

    programs = {
      bash.interactiveShellInit = optionalString cfg.hooks.bash.enable ''
        # Aliases added by EH
        alias nr='eh run'
        alias ns='eh shell'
        alias nb='eh build'
        alias nu='eh update'
      '';

      zsh.interactiveShellInit = optionalString cfg.hooks.zsh.enable ''
        # Aliases added by EH
        alias nr='eh run'
        alias ns='eh shell'
        alias nb='eh build'
        alias nu='eh update'
      '';
    };
  };
}
