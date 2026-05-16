self: {
  config,
  pkgs,
  lib,
  ...
}: let
  inherit (lib.modules) mkIf;
  inherit (lib.options) mkEnableOption mkPackageOption literalExpression;
  inherit (lib.strings) optionalString;

  cfg = config.programs.eh;
in {
  options.programs.eh = {
    enable = mkEnableOption "eh - Ergonomic Nix CLI helper";
    package = mkPackageOption self.packages.${pkgs.stdenv.hostPlatform.system} ["eh"] {
      pkgsText = literalExpression "self.packages.$${pkgs.stdenv.hostPlatform.system}";
    };

    hooks = {
      bash.enable = mkEnableOption "Bash shell hook for EH" // {default = config.programs.bash.enable;};
      zsh.enable = mkEnableOption "ZSH shell hook for EH" // {default = config.programs.zsh.enable;};
      fish.enable = mkEnableOption "Fish shell hook for EH" // {default = config.programs.fish.enable;};
    };
  };

  config = mkIf cfg.enable {
    environment.systemPackages = [cfg.package];
    programs = {
      bash.interactiveShellInit = optionalString cfg.hooks.bash.enable ''
        # EH multicall aliases
        alias nr='eh run'
        alias ns='eh shell'
        alias nb='eh build'
        alias nd='eh develop'
        alias ni='eh info'
        alias nu='eh update'
        # End of EH aliases
      '';

      zsh.interactiveShellInit = optionalString cfg.hooks.zsh.enable ''
        # EH multicall aliases
        alias nr='eh run'
        alias ns='eh shell'
        alias nb='eh build'
        alias nd='eh develop'
        alias ni='eh info'
        alias nu='eh update'
        # End of EH aliases
      '';

      fish.interactiveShellInit = optionalString cfg.hooks.fish.enable ''
        # EH multicall aliases
        alias nr='eh run'
        alias ns='eh shell'
        alias nb='eh build'
        alias nd='eh develop'
        alias ni='eh info'
        alias nu='eh update'
        # End of EH aliases
      '';
    };
  };
}
