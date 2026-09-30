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
            targets = [ "wasm32-unknown-unknown" ];
          };

          craneLib = (crane.mkLib pkgs).overrideToolchain (_: rustToolchain);

          src = craneLib.cleanCargoSource ./.;

          # crates/litgraph/build.rs embeds these data directories into the
          # binary at build time, so the crane source filter keeps them
          # alongside the Rust sources.
          dataDirs = [ "packs" "scenarios" "calibration" "sources" "examples" "tests" ];
          unfilteredSrc = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              (craneLib.filterCargoSources path type)
              || builtins.any (d: pkgs.lib.hasInfix "/${d}/" path || baseNameOf path == d) dataDirs;
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

          litgraphMcp = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              pname = "litgraph-mcp";
              cargoExtraArgs = "-p litgraph-mcp";
              doCheck = false; # `nix flake check` runs the real test suite separately.

              meta = with pkgs.lib; {
                description = "MCP (Model Context Protocol) stdio server for litgraph: every engine op as a tool, packs/links.json/the manual as resources.";
                homepage = "https://github.com/davidawad/litgraph";
                license = licenses.gpl3Plus;
                mainProgram = "litgraph-mcp";
              };
            }
          );

          # --- wasm: litgraph-wasm for web + nodejs, via wasm-bindgen -----------
          wasmArgs = commonArgs // {
            pname = "litgraph-wasm";
            cargoExtraArgs = "-p litgraph-wasm --target wasm32-unknown-unknown";
            CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
            doCheck = false; # host can't execute wasm32 test binaries without wasmtime
          };
          wasmCargoArtifacts = craneLib.buildDepsOnly wasmArgs;
          litgraphWasmRaw = craneLib.buildPackage (
            wasmArgs // { cargoArtifacts = wasmCargoArtifacts; }
          );

          # wasm-bindgen-cli must match the `wasm-bindgen` crate version pinned in
          # crates/litgraph-wasm/Cargo.toml (=0.2.127) exactly -- a mismatch fails
          # at wasm-bindgen invocation time, not at cargo build time.
          litgraphWasm = pkgs.stdenv.mkDerivation {
            pname = "litgraph-wasm";
            version = "0.1.0";
            src = litgraphWasmRaw;
            nativeBuildInputs = [ pkgs.wasm-bindgen-cli ];
            buildPhase = ''
              runHook preBuild
              mkdir -p $out/web $out/node
              wasm_file=$(find $src -name "litgraph_wasm.wasm" | head -1)
              wasm-bindgen --target web --out-dir $out/web "$wasm_file"
              wasm-bindgen --target nodejs --out-dir $out/node "$wasm_file"
              runHook postBuild
            '';
            dontInstall = true;
            dontFixup = true;
          };

          devTools = with pkgs; [
            rustToolchain
            cargo-nextest
            cargo-llvm-cov
            cargo-audit
            cargo-deny
            cargo-machete
            just
            jq
            wasm-bindgen-cli
            wasm-pack
            # kani is not packaged in nixpkgs; install via `cargo install
            # kani-verifier && cargo kani setup` outside the sandbox, or run
            # `just verify-kani` from a non-nix shell / CI's cargo-kani action.
            # `cargo install wasm-pack` fails to link locally (-lbz2 missing)
            # on at least one dev machine -- use this nix-provided binary.
          ];
        in
        {
          packages.default = litgraph;
          packages.litgraph = litgraph;
          packages.litgraph-mcp = litgraphMcp;
          packages.wasm = litgraphWasm;

          apps.default = flake-utils.lib.mkApp { drv = litgraph; };
          apps.litgraph-mcp = flake-utils.lib.mkApp { drv = litgraphMcp; };

          devShells.default = pkgs.mkShell {
            packages = devTools;
            shellHook = ''
              export LITGRAPH_PACKS="$PWD/packs"
            '';
          };

          checks = {
            build = litgraph;
            build-mcp = litgraphMcp;

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
