let
  pins = import ./npins;
  pkgs = import pins.nixpkgs { overlays = [ (import pins.rust-overlay) ]; };
  inherit (pkgs) lib;
in
pkgs.mkShell {
  packages = [
    (pkgs.rust-bin.selectLatestNightlyWith (
      toolchain:
      toolchain.default.override {
        extensions = [ "rust-src" ];
      }
    ))
    pkgs.npins
    pkgs.nixfmt
  ];
}
