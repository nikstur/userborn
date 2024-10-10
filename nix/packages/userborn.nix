{
  lib,
  rustPlatform,
  libxcrypt,
}:

let
  cargoToml = builtins.fromTOML (builtins.readFile ../../rust/userborn/Cargo.toml);
in
rustPlatform.buildRustPackage {
  pname = cargoToml.package.name;
  inherit (cargoToml.package) version;

  src = lib.sourceFilesBySuffices ../../rust/userborn [
    ".rs"
    ".toml"
    ".lock"
  ];

  cargoLock = {
    lockFile = ../../rust/userborn/Cargo.lock;
  };

  nativeBuildInputs = [
    rustPlatform.bindgenHook
  ];

  buildInputs = [
    libxcrypt
  ];

  stripAllList = [ "bin" ];

  meta = with lib; {
    homepage = "https://github.com/nikstur/userborn";
    license = licenses.mit;
    maintainers = with lib.maintainers; [ nikstur ];
    mainProgram = "userborn";
  };
}
