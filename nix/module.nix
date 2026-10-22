# NixOS module for Zynveliq voice assistant
#
# Handles system-level configuration that the package wrapper cannot:
#   - udev rule for /dev/uinput (rdev grab() needs it for virtual input)
#
# Note: users must add themselves to the "input" group for evdev hotkey access.
#
# Usage in your flake:
#
#   inputs.zynveliq.url = "github:ishangautam7/zynveliq";
#
#   nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
#     modules = [
#       zynveliq.nixosModules.default
#       { programs.zynveliq.enable = true; }
#     ];
#   };
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.zynveliq;
in
{
  options.programs.zynveliq = {
    enable = lib.mkEnableOption "Zynveliq offline voice assistant";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "zynveliq.packages.\${system}.zynveliq";
      description = "The Zynveliq package to use.";
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
