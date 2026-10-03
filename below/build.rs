// Copyright (c) Facebook, Inc. and its affiliates.
// Copyright (c) 2026 Jaroslav Pachola
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// Set to a prebuilt exitstat BPF object to use it instead of building one,
/// e.g. where no nightly toolchain or bpf-linker is available.
const PREBUILT_ENV: &str = "BELOW_EXITSTAT_BPF_OBJ";

fn main() {
    let out_dir =
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR must be set in build script"));
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set in build script"),
    );

    println!("cargo:rerun-if-env-changed={PREBUILT_ENV}");
    let object = match env::var_os(PREBUILT_ENV) {
        Some(path) => {
            let path = PathBuf::from(path);
            println!("cargo:rerun-if-changed={}", path.display());
            path
        }
        None => build_exitstat_bpf(&manifest_dir, &out_dir),
    };

    // src/exitstat.rs embeds the object from this fixed path.
    fs::copy(&object, out_dir.join("exitstat.bpf.o")).unwrap_or_else(|e| {
        panic!(
            "Failed to copy exitstat BPF object {}: {e}",
            object.display()
        )
    });
}

/// Build the exitstat-ebpf crate for the BPF target and return the object.
///
/// That crate pins its own nightly toolchain (rust-toolchain.toml) and target
/// settings (.cargo/config.toml), which rustup's `cargo` picks up from its
/// directory. Everything Cargo set for this build is cleared first so none of
/// it (toolchain, rustflags, target dir, wrappers) leaks into that one.
fn build_exitstat_bpf(manifest_dir: &Path, out_dir: &Path) -> PathBuf {
    let crate_dir = manifest_dir.join("exitstat-ebpf");
    let target_dir = out_dir.join("exitstat-ebpf");
    for path in [
        crate_dir.join("src"),
        crate_dir.join("Cargo.toml"),
        crate_dir.join("Cargo.lock"),
        crate_dir.join("rust-toolchain.toml"),
        crate_dir.join(".cargo/config.toml"),
        manifest_dir.join("exitstat-common/src"),
        manifest_dir.join("exitstat-common/Cargo.toml"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let mut cmd = Command::new("cargo");
    cmd.current_dir(&crate_dir)
        .args(["build", "--release", "--locked", "--target-dir"])
        .arg(&target_dir);
    for (key, _) in env::vars_os() {
        if inherited_from_cargo(&key) {
            cmd.env_remove(key);
        }
    }

    let status = cmd
        .status()
        .unwrap_or_else(|e| panic!("Failed to run cargo for {}: {e}", crate_dir.display()));
    if !status.success() {
        panic!(
            "Failed to build the exitstat BPF program in {}. It needs rustup with the \
             toolchain pinned in its rust-toolchain.toml (rustup installs it) and bpf-linker \
             (`cargo install bpf-linker`). Alternatively set {PREBUILT_ENV} to a prebuilt object.",
            crate_dir.display()
        );
    }
    target_dir.join("bpfel-unknown-none/release/exitstat")
}

fn inherited_from_cargo(key: &OsStr) -> bool {
    let key = key.to_string_lossy();
    let keep = key == "CARGO_HOME" || key == "RUSTUP_HOME";
    !keep && (key.starts_with("CARGO") || key.starts_with("RUST"))
}
