# Installation

← [Back to README](../README.md)

## Installation

### Prerequisites

- Rust toolchain (stable; the `edition = "2021"` crate).
- macOS, Linux, or Windows (x86_64 or ARM64).

### From source

```bash
git clone https://github.com/Miyo-explore/miyo-rpc-labs.git
cd miyo-rpc-labs
cargo build --release
```

The binary will be at `target/release/miyo-rpc`.

### Prebuilt binaries

GitHub Releases ship checksummed `.tar.gz` artifacts per platform, named
`miyo-rpc-v<VERSION>-<target>.tar.gz` (plus a `.sha256` sidecar). Pick the
`<target>` matching your machine:

| Platform | `<target>` |
|----------|------------|
| Linux x86_64 | `x86_64-unknown-linux-gnu` |
| macOS Apple Silicon | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |
| Windows x86_64 | `x86_64-pc-windows-msvc` |

Verify the checksum before extracting. **Linux x86_64** (the binary inside is
`miyo-rpc`):

```bash
VERSION=0.6.0
TARGET=x86_64-unknown-linux-gnu
curl -L "https://github.com/Miyo-explore/miyo-rpc-labs/releases/download/v${VERSION}/miyo-rpc-v${VERSION}-${TARGET}.tar.gz" -o "miyo-rpc-v${VERSION}-${TARGET}.tar.gz"
curl -L "https://github.com/Miyo-explore/miyo-rpc-labs/releases/download/v${VERSION}/miyo-rpc-v${VERSION}-${TARGET}.tar.gz.sha256" -o "miyo-rpc-v${VERSION}-${TARGET}.tar.gz.sha256"
sha256sum -c "miyo-rpc-v${VERSION}-${TARGET}.tar.gz.sha256" && tar -xzf "miyo-rpc-v${VERSION}-${TARGET}.tar.gz" && ./miyo-rpc --version
```

**macOS** (Apple Silicon or Intel; the binary inside is `miyo-rpc`):

```bash
VERSION=0.6.0
TARGET=aarch64-apple-darwin   # or x86_64-apple-darwin for Intel
curl -L "https://github.com/Miyo-explore/miyo-rpc-labs/releases/download/v${VERSION}/miyo-rpc-v${VERSION}-${TARGET}.tar.gz" -o "miyo-rpc-v${VERSION}-${TARGET}.tar.gz"
curl -L "https://github.com/Miyo-explore/miyo-rpc-labs/releases/download/v${VERSION}/miyo-rpc-v${VERSION}-${TARGET}.tar.gz.sha256" -o "miyo-rpc-v${VERSION}-${TARGET}.tar.gz.sha256"
shasum -a 256 -c "miyo-rpc-v${VERSION}-${TARGET}.tar.gz.sha256" && tar -xzf "miyo-rpc-v${VERSION}-${TARGET}.tar.gz" && ./miyo-rpc --version
```

**Windows** x86_64 (`miyo-rpc.exe` inside; `tar` is built into Windows 10/11):

```powershell
$VERSION = "0.6.0"
$TARGET  = "x86_64-pc-windows-msvc"
Invoke-WebRequest "https://github.com/Miyo-explore/miyo-rpc-labs/releases/download/v$VERSION/miyo-rpc-v$VERSION-$TARGET.tar.gz" -OutFile "miyo-rpc-v$VERSION-$TARGET.tar.gz"
Invoke-WebRequest "https://github.com/Miyo-explore/miyo-rpc-labs/releases/download/v$VERSION/miyo-rpc-v$VERSION-$TARGET.tar.gz.sha256" -OutFile "miyo-rpc-v$VERSION-$TARGET.tar.gz.sha256"
$expected = (Get-Content "miyo-rpc-v$VERSION-$TARGET.tar.gz.sha256" -Raw).Split()[0].Trim().ToLower()
$actual = (Get-FileHash "miyo-rpc-v$VERSION-$TARGET.tar.gz" -Algorithm SHA256).Hash.ToLower()
if ($expected -ne $actual) { "checksum MISMATCH"; exit 1 } else { "checksum OK" }
tar -xzf "miyo-rpc-v$VERSION-$TARGET.tar.gz"; .\miyo-rpc.exe --version
```

The single binary has no runtime dependencies; building from source (see
Prerequisites) works on macOS, Linux, and Windows.
