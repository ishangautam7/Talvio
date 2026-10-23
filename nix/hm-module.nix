# Home-manager module for Talvio voice assistant
#
# Provides a systemd user service for autostart.
# Usage: imports = [ talvio.homeManagerModules.default ];
#        services.talvio.enable = true;
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.talvio;
in
{
  # Renamed from Zynveliq: old configurations keep working, with a warning.
  imports = [
    (lib.mkRenamedOptionModule [ "services" "zynveliq" "enable" ] [ "services" "talvio" "enable" ])
    (lib.mkRenamedOptionModule [ "services" "zynveliq" "package" ] [ "services" "talvio" "package" ])
  ];

  options.services.talvio = {
    enable = lib.mkEnableOption "Talvio voice assistant user service";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "talvio.packages.\${system}.talvio";
      description = "The Talvio package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.talvio = {
      Unit = {
        Description = "Talvio voice assistant";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${cfg.package}/bin/talvio";
        Restart = "on-failure";
        RestartSec = 5;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
