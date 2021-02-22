{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    rustc
    cargo
    rustfmt
  ];

  RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
}
