{
  description = "litgraph — litigation procedure graphs as stochastic games";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
      crane,
    }:
    flake-utils.lib.eachSystem
      [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ]
      (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };

          # Match rust-toolchain.toml exactly so `nix build` and `cargo build`
          # use the same compiler.
          rustToolchain = pkgs.rust-bin.stable."1.97.1".default.override {
            extensions = [
              "rustfmt"
              "clippy"
              "llvm-tools-preview"
            ];
          };

          craneLib = (crane.mkLib pkgs).overrideToolchain (_: rustToolchain);

          src = craneLib.cleanCargoSource ./.;

          # The workspace build needs packs/ present as source (a build.rs in
          # crates/litgraph embeds packs/ into the binary at build time), so
          # the crane source filter is widened beyond just Rust files.
          unfilteredSrc = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              (craneLib.filterCargoSources path type) || (pkgs.lib.hasInfix "/packs/" path) || (baseNameOf path == "packs");
          };

          commonArgs = {
            src = unfilteredSrc;
            strictDeps = true;
            nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.libiconv
            ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          litgraph = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              pname = "litgraph";
              cargoExtraArgs = "-p litgraph-cli";
              doCheck = false; # `nix flake check` runs the real test suite separately.

              postInstall = ''
                mkdir -p $out/share/litgraph
                cp -r packs $out/share/litgraph/packs
              '';

              meta = with pkgs.lib; {
                description = "Litigation procedure graphs as stochastic games: load, compose, weight, solve, simulate.";
                homepage = "https://github.com/davidawad/litgraph";
                license = licenses.gpl3Plus;
                mainProgram = "litgraph";
              };
            }
          );

          devTools = with pkgs; [
            rustToolchain
            cargo-nextest
            cargo-llvm-cov
            cargo-audit
            cargo-deny
            cargo-machete
            just
            jq
            # kani is not packaged in nixpkgs; install via `cargo install
            # kani-verifier && cargo kani setup` outside the sandbox, or run
            # `just verify-kani` from a non-nix shell / CI's cargo-kani action.
          ];
        in
        {
          packages.default = litgraph;
          packages.litgraph = litgraph;

          apps.default = flake-utils.lib.mkApp { drv = litgraph; };

          devShells.default = pkgs.mkShell {
            packages = devTools;
            shellHook = ''
              export LITGRAPH_PACKS="$PWD/packs"
            '';
          };

          checks = {
            build = litgraph;

            clippy = craneLib.cargoClippy (
              commonArgs
              // {
                inherit cargoArtifacts;
                cargoClippyExtraArgs = "--workspace --all-targets -- -D warnings";
              }
            );

            fmt = craneLib.cargoFmt { inherit src; };

            nextest = craneLib.cargoNextest (
              commonArgs
              // {
                inherit cargoArtifacts;
                partitions = 1;
                partitionType = "count";
              }
            );
          };

          formatter = pkgs.nixfmt-rfc-style;
        }
      );
}
