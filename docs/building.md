# Dependencies

* rustup, with the stable toolchain. The exitstat BPF program is written in
  Rust and builds with the nightly toolchain pinned in
  `below/exitstat-ebpf/rust-toolchain.toml` (with its `rust-src` component);
  rustup installs it on first use.
* [bpf-linker](https://github.com/aya-rs/bpf-linker), which links the BPF
  program. Install a prebuilt release, or build it with `cargo install
  bpf-linker` (that needs LLVM development libraries).
* A C compiler, for the zstd library that the store's compression uses.

## Install build dependencies

### Ubuntu

```shell
sudo apt install -y build-essential ca-certificates curl git pkg-config
# rustup: https://rustup.rs
curl -sSL https://github.com/aya-rs/bpf-linker/releases/download/v0.11.1/bpf-linker-x86_64-unknown-linux-musl.tar.zst \
  | tar --zstd -x -C ~/.cargo/bin
```

### Without nightly Rust or bpf-linker

Set `BELOW_EXITSTAT_BPF_OBJ` to an exitstat BPF object built elsewhere (found at
`target/<profile>/build/below-*/out/exitstat.bpf.o` after a normal build) and
the build uses it instead of building one.

## Optional: BPF cgroup stats (`cgroup-bpf` feature)

The `enable_cgroup_bpf` config option needs belower built with
`--features cgroup-bpf`. That feature builds libbpf and a BPF program written in
C, so it additionally needs clang-15+, libelf, zlib and rustfmt:

```shell
sudo apt install -y clang libelf-dev zlib1g-dev
rustup component add rustfmt
```

Without the feature, belower ignores `enable_cgroup_bpf` (it logs why) and reads
the cgroup files.

# Building

belower's UI is quite laggy in debug builds. We recommend always building in
release mode.

In the root of the repository:

```shell
cargo build --release
cargo test
```
