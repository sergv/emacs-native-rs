{
  description = "Rust module to be called by Emacs";

  inputs = {
    nixpkgs = {
      # # unstable
      # url = "nixpkgs/nixos-unstable";
      #url = "nixpkgs/nixos-22.05";
      url = "/home/sergey/nix/nixpkgs";
    };

    # nixpkgs.url = "nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    # rust-overlay = {
    #   url = "github:oxalica/rust-overlay";
    #   inputs.nixpkgs.follows = "nixpkgs";
    # };

    # pre-commit-hooks.url = "github:cachix/pre-commit-hooks.nix";
    # gitignore = {
    #   url = "github:hercules-ci/gitignore.nix";
    #   inputs.nixpkgs.follows = "nixpkgs";
    # };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    # gitignore,
    # rust-overlay,
    # pre-commit-hooks,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        # overlays = [
        #   rust-overlay.overlays.default
        # ];
      };

      # inherit (gitignore.lib) gitignoreSource;
      # pre-commit-check = pre-commit-hooks.lib.${system}.run {
      #   src = gitignoreSource ./.;
      #   hooks = {
      #     alejandra.enable = true;
      #     rustfmt.enable = true;
      #   };
      # };

      rust = pkgs.rust-bin.stable.latest.default;

      # rustWasm = rust.override {
      #   targets = ["wasm32-unknown-unknown"];
      # };

      emacs_module_rs_package = pkgs.rustPlatform.buildRustPackage {
        pname = "emacs_module_rs";
        version = "0.1.0";

        src = ./.;
        cargoLock = {
          lockFile = ./Cargo.lock;
        };
        buildInputs = [
          pkgs.libclang
        ];

        # nativeBuildInputs = [rust pkgs.makeWrapper];
        # postInstall = ''
        #   wrapProgram "$out/bin/egui-playground-bin" --prefix LD_LIBRARY_PATH : "${libPath}"
        # '';
      };

      # # Required by egui
      # libPath = with pkgs;
      #   lib.makeLibraryPath [
      #     libGL
      #     libxkbcommon
      #     wayland
      #     xorg.libX11
      #     xorg.libXcursor
      #     xorg.libXi
      #     xorg.libXrandr
      #   ];

    in {
      packages = {
        emacs_module_rs = emacs_module_rs_package;
        default         = emacs_module_rs_package;
      };
      devShells = {
        default = pkgs.mkShell rec {
          buildInputs = [pkgs.rustfmt pkgs.rustc pkgs.cargo];
          # inherit (pre-commit-check) shellHook;
          # LD_LIBRARY_PATH = libPath;
          NIX_DEVELOP_PROMPT = "[nix]";
        };
      };
      # apps = {
      #   gui = {
      #     type = "app";
      #     program = "${packages.default}/bin/egui-playground-bin";
      #   };
      #   default = apps.gui;
      # };
    });
}
