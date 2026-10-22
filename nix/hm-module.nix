# Home-manager module for Zynveliq voice assistant
#
# Provides a systemd user service for autostart.
# Usage: imports = [ zynveliq.homeManagerModules.default ];
#        services.zynveliq.enable = true;
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.zynveliq;
in
{
  options.services.zynveliq = {
    enable = lib.mkEnableOption "Zynveliq voice assistant user service";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "zynveliq.packages.\${system}.zynveliq";
      description = "The Zynveliq package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.zynveliq = {
      Unit = {
        Description = "Zynveliq voice assistant";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${cfg.package}/bin/zynveliq";
        Restart = "on-failure";
        RestartSec = 5;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
