# Zeus UI Components

This crate contains reusable egui UI components made for Zeus.

## Features

- `qr-scanner`: Enables the QR scanner component
- `secure-types`: Enables the components that use secure types

## Linux System Requirements

QR scanner uses the xcap crate, which requires the following dependencies:

Debian/Ubuntu:

```bash
apt-get install pkg-config libclang-dev libxcb1-dev libxrandr-dev libdbus-1-dev libpipewire-0.3-dev libwayland-dev libegl-dev
```

Alpine:

```bash
apk add pkgconf llvm19-dev clang19-dev libxcb-dev libxrandr-dev dbus-dev pipewire-dev wayland-dev mesa-dev
```

Arch Linux:

```bash
pacman -S base-devel clang libxcb libxrandr dbus libpipewire
```

### If it still doesn't compile, try installing the following packages:

```bash
apt install libgbm-dev libdrm-dev libgl1-mesa-dev
```