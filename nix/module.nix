# NixOS module for Talvio voice assistant
#
# Handles system-level configuration that the package wrapper cannot:
#   - udev rule for /dev/uinput (rdev grab() needs it for virtual input)
#
# Note: users must add themselves to the "input" group for evdev hotkey access.
#
# Usage in your flake:
#
#   inputs.talvio.url = "github:ishangautam7/zynveliq";
#
#   nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
#     modules = [
#       talvio.nixosModules.default
#       { programs.talvio.enable = true; }
#     ];
#   };
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.talvio;
in
{
  # Renamed from Zynveliq: old configurations keep working, with a warning.
  imports = [
    (lib.mkRenamedOptionModule [ "programs" "zynveliq" "enable" ] [ "programs" "talvio" "enable" ])
    (lib.mkRenamedOptionModule [ "programs" "zynveliq" "package" ] [ "programs" "talvio" "package" ])
  ];

  options.programs.talvio = {
    enable = lib.mkEnableOption "Talvio offline voice assistant";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "talvio.packages.\${system}.talvio";
      description = "The Talvio package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];

    # rdev grab() creates virtual input devices via /dev/uinput.
    # Default permissions are crw------- root root — open it to the input group.
    services.udev.extraRules = ''
      KERNEL=="uinput", GROUP="input", MODE="0660"
    '';
  };
}
