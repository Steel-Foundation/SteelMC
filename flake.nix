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

          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };

          # The build script normally downloads this jar, but Nix builds have no
          # internet access, so it is fetched here instead.
          # Careful: this is Mojang's file. Never upload it to a public Nix cache.
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
          inherit pkgs toolchain steel;
        }
      );
    in
    {
      devShells = lib.mapAttrs (_: system: {
        default = system.pkgs.mkShell {
          packages = [
            system.toolchain

            system.pkgs.lld

            system.pkgs.prek
            system.pkgs.typos

            system.pkgs.git
            system.pkgs.jdk25
          ];
        };
      }) perSystem;

      packages = lib.mapAttrs (_: system: { default = system.steel; }) perSystem;

      checks = lib.genAttrs linuxSystems (system: {
        package = perSystem.${system}.steel;
      });

      formatter = lib.mapAttrs (_: system: system.pkgs.nixfmt-tree) perSystem;
    };
}
