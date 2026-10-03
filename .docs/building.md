# Building from source

Requirements:

- **Rust** — stable toolchain, edition 2024
- **CMake** and a C toolchain — Opus and AWS-LC are compiled from source
- **Node.js 20+** — builds the remote-control web UI embedded into the binary

Platform libraries:

**Windows** — MSVC C++ build tools (Desktop development with C++); NASM on
x64, `clang-cl` on ARM64:

```sh
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install Kitware.CMake
winget install NASM.NASM
```

**macOS** — Xcode Command Line Tools, Metal toolchain (GPUI), CMake:

```sh
xcodebuild -downloadComponent MetalToolchain
brew install cmake
```

**Linux** — Debian/Ubuntu:

```sh
sudo apt-get install -y libasound2-dev libfontconfig-dev libwayland-dev \
  libxkbcommon-x11-dev libssl-dev libzstd-dev libgit2-dev \
  build-essential cmake clang
```

## Build

```sh
# remote-control web UI (build before the app)
cd web && npm ci && npm run build && cd ..
cargo run --release
```

Release binary: `target/release/pawse` (`pawse.exe` on Windows).
