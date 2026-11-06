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

        # nixpkgs lags behind Biome releases, so take upstream's static (musl)
        # binary, which runs on NixOS as is. Bump version and hash together.
        biome = let version = "2.6.2"; in pkgs.runCommand "biome-${version}" {
            src = pkgs.fetchurl {
                url = "https://github.com/biomejs/biome/releases/download/%40biomejs/biome%40${version}/biome-linux-x64-musl";
                hash = "sha256-67PQ6Ay+5QPEwZpwiR3XWJ4i6AFBJjCgytQCsZynjJM=";
            };
        } "install -Dm755 $src $out/bin/biome";

        sharedPackages = [
            rustToolchain
            pkgs.rust-analyzer
            pkgs.cargo-machete
            pkgs.gh

            pkgs.nats-server
            # `stripe listen` forwards test webhooks to a local API.
            pkgs.stripe-cli

            pkgs.nodejs_24
            pkgs.pnpm
            biome

            pkgs.pkg-config
            pkgs.openssl
        ];

        sharedEnv = {
            RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
            # The npm Biome binary is dynamically linked and won't run on NixOS;
            # point its wrapper at the one above. Keep the version in
            # web/dashboard/package.json in sync with it.
            BIOME_BINARY = "${biome}/bin/biome";
        };

        connectPackages = with pkgs; [
            gobject-introspection
            at-spi2-atk
            atkmm
            cairo
            gdk-pixbuf
            glib
            glib-networking
            gtk3
            harfbuzz
            librsvg
            libsoup_3
            pango
            webkitgtk_4_1
            wrapGAppsHook4

            xdotool
            libayatana-appindicator
        ];
    in {
        devShells.x86_64-linux.default = pkgs.mkShell {
            packages = sharedPackages;
            env = sharedEnv;
        };

        devShells.x86_64-linux.connect = pkgs.mkShell {
            packages = sharedPackages ++ connectPackages;
            env = sharedEnv // {
                LD_LIBRARY_PATH =
                    pkgs.lib.makeLibraryPath [ pkgs.libayatana-appindicator ];
            };
            shellHook = ''
                export XDG_DATA_DIRS="$GSETTINGS_SCHEMAS_PATH''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
            '';
        };
    };
}
