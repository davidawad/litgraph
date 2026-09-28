{ pkgs, lib, ... }:

{
  # Same toolchain as rust-toolchain.toml / flake.nix.
  languages.rust = {
    enable = true;
    channel = "stable";
    version = "1.97.1";
    components = [
      "rustc"
      "cargo"
      "clippy"
      "rustfmt"
      "rust-src"
      "llvm-tools-preview"
    ];
  };

  packages = with pkgs; [
    cargo-nextest
    cargo-llvm-cov
    cargo-audit
    cargo-deny
    cargo-machete
    just
    jq
    # kani is not in nixpkgs; `cargo install kani-verifier && cargo kani
    # setup` outside this shell (or CI's model-checking/kani-github-action)
    # to run `just verify-kani`.
  ];

  env.LITGRAPH_PACKS = "${builtins.toString ./.}/packs";

  scripts.fmt.exec = "just fmt";
  scripts.fmt-check.exec = "just fmt-check";
  scripts.lint.exec = "just lint";
  scripts.test.exec = "just test";
  scripts.doctest.exec = "just doctest";
  scripts.cov.exec = "just cov";
  scripts.audit.exec = "just audit";
  scripts.deny.exec = "just deny";
  scripts.ci.exec = "just ci";

  git-hooks.hooks = {
    rustfmt = {
      enable = true;
      entry = lib.mkForce "cargo fmt --all -- --check";
      pass_filenames = false;
    };
    clippy = {
      enable = true;
      entry = lib.mkForce "cargo clippy --workspace --all-targets -- -D warnings";
      pass_filenames = false;
    };
  };

  enterShell = ''
    echo "litgraph devenv: rustc $(rustc --version | cut -d' ' -f2), just $(just --version | cut -d' ' -f2)"
    echo "run 'just' for available recipes, 'just ci' before pushing."
  '';
}
