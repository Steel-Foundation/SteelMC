{
  description = "SteelMC development environment and server package";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      inherit (nixpkgs) lib;

      # nixpkgs dropped x86_64-darwin in 26.11; evaluating it throws.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      linuxSystems = lib.filter (lib.hasSuffix "-linux") systems;

      forAllSystems =
        f:
        lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
            }
          )
        );

      assets = builtins.fromJSON (builtins.readFile ./nix/minecraft-assets.json);

      perSystem = forAllSystems (
        pkgs:
        let
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

          # rust-analyzer needs rust-src; rust-toolchain.toml only pins the channel.
          devToolchain = toolchain.override {
            extensions = [
              "rust-src"
              "rust-analyzer"
            ];
          };

          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };

          # Fetched here since Nix builds have no network access. Mojang's file — never upload to a public cache.
          serverJar = pkgs.fetchurl { inherit (assets.serverJar) url hash; };

          buildAssets =
            pkgs.runCommand "steel-build-assets-${assets.minecraftVersion}"
              {
                nativeBuildInputs = [
                  pkgs.unzip
                  pkgs.jq
                ];
              }
              ''
                unzip -qq ${serverJar} -d outer

                jarVersion=$(jq -r '.id' outer/version.json)
                if [ "$jarVersion" != "${assets.minecraftVersion}" ]; then
                  echo "error: pinned server jar is Minecraft $jarVersion, but the" >&2
                  echo "targeted version is ${assets.minecraftVersion}." >&2
                  echo "Run ./update-minecraft-assets.sh to regenerate the pin." >&2
                  exit 1
                fi

                nested=$(find outer/META-INF/versions -name '*.jar' -print -quit 2>/dev/null || true)
                if [ -n "$nested" ]; then
                  unzip -qq "$nested" -d inner
                else
                  mv outer inner
                fi

                mkdir -p "$out/builtin_datapacks"
                cp -r inner/data/minecraft "$out/builtin_datapacks/minecraft"
                cp inner/assets/minecraft/lang/en_us.json "$out/en_us.json"
                cp inner/assets/minecraft/lang/deprecated.json "$out/deprecated.json"

                chmod -R u+w "$out"
                printf '%s' "${assets.minecraftVersion}" > "$out/builtin_datapacks/minecraft/.version"
              '';

          steel = rustPlatform.buildRustPackage {
            pname = "steel";
            inherit (assets) version;

            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "text_components-0.1.7" = "sha256-cGVxp7QX9qYuPl/NW+ya889cCmj5D/bV7tW5SjzBohs=";
              };
            };

            nativeBuildInputs = [ pkgs.lld ];

            # Lets the build script's assets_are_valid check short-circuit before it
            # reaches any network call.
            preBuild = ''
              mkdir -p steel-utils/build_assets
              cp -r --no-preserve=mode,ownership ${buildAssets}/. steel-utils/build_assets/
            '';

            cargoBuildFlags = [
              "--package"
              "steel"
            ];

            # Suite has known flakiness under constrained parallelism; run tests via `cargo test` instead.
            doCheck = false;

            meta = {
              description = "Minecraft server implementation written in Rust";
              homepage = "https://steelmc.dev";
              license = lib.licenses.agpl3Plus;
              mainProgram = "steel";
              platforms = linuxSystems;
            };
          };
        in
        {
          inherit
            pkgs
            toolchain
            devToolchain
            steel
            ;
        }
      );
    in
    {
      devShells = lib.mapAttrs (_: system: {
        default = system.pkgs.mkShell {
          packages = [
            system.devToolchain

            system.pkgs.lld

            system.pkgs.prek
            system.pkgs.typos

            system.pkgs.git
            system.pkgs.jdk25
          ];
        };
      }) perSystem;

      # Scoped to linuxSystems: steel's meta.platforms excludes aarch64-darwin, and offering it anyway breaks `nix flake check`.
      packages = lib.genAttrs linuxSystems (system: {
        default = perSystem.${system}.steel;
      });

      checks = lib.genAttrs linuxSystems (system: {
        package = perSystem.${system}.steel;
      });

      formatter = lib.mapAttrs (_: system: system.pkgs.nixfmt-tree) perSystem;
    };
}
