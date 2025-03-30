{ pkgs }: {
  deps = [
    pkgs.pkg-config
    pkgs.wayland
    pkgs.wayland-protocols
  ];
}
