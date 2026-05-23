# Nix dependency declaration for the matchbox_server build.
# Replit resolves these via its Nix channel (see `.replit`).
{ pkgs }: {
  deps = [
    # Rust toolchain for `cargo install matchbox_server`.
    pkgs.cargo
    pkgs.rustc
    pkgs.rustfmt
    # Build deps that matchbox_server's transitive crates
    # (axum / tower / openssl) need to link against.
    pkgs.pkg-config
    pkgs.openssl
    pkgs.openssl.dev
  ];
  env = {
    # Help openssl-sys find the right libs at build time.
    OPENSSL_DIR = "${pkgs.openssl.dev}";
    OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
    OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
    PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
  };
}
