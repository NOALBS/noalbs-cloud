{
    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
        rust-overlay.url = "github:oxalica/rust-overlay";
    };

    outputs = { self, nixpkgs, rust-overlay }: let 
        pkgs = import nixpkgs {
            system = "x86_64-linux";
            overlays = [ rust-overlay.overlays.default ];
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" ];
            targets = [ "wasm32-unknown-unknown" ];
        };
    in {
        devShells.x86_64-linux.default = pkgs.mkShell {
            packages = [
                rustToolchain
                pkgs.rust-analyzer
                pkgs.cargo-machete

                pkgs.nats-server
                # `stripe listen` forwards test webhooks to a local API.
                pkgs.stripe-cli

                pkgs.nodejs_24
                pkgs.pnpm
                pkgs.biome

                pkgs.pkg-config
                pkgs.openssl
            ];
            env.RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
            # The npm Biome binary is dynamically linked and won't run on NixOS;
            # point its wrapper at the native one. Keep the version in
            # web/dashboard/package.json in sync with this nixpkgs.
            env.BIOME_BINARY = "${pkgs.biome}/bin/biome";
        };
    };
}