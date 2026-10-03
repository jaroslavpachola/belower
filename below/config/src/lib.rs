// Copyright (c) Facebook, Inc. and its affiliates.
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

#![deny(clippy::all)]

use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::Result;
use anyhow::bail;
pub use procfs::ProcessStackTraceFilter;
use serde::Deserialize;
use serde::Serialize;

#[cfg(test)]
mod test;

/// System-wide locations, used when running as root.
pub const SYSTEM_CONF: &str = "/etc/belower/belower.conf";
pub const SYSTEM_STORE: &str = "/var/log/belower/store";

/// Default file and directory locations. As root they are system-wide; as any
/// other user they are per-user, under the XDG base directories, so that
/// recording and replaying work without root.
#[derive(Debug, PartialEq)]
pub struct Locations {
    pub config: PathBuf,
    pub log_dir: PathBuf,
    pub store_dir: PathBuf,
}

impl Locations {
    /// The locations for the current user and environment.
    pub fn current() -> Self {
        // SAFETY: geteuid has no preconditions and cannot fail.
        let is_root = unsafe { libc::geteuid() } == 0;
        Self::resolve(is_root, |var| std::env::var_os(var))
    }

    fn resolve(is_root: bool, env: impl Fn(&str) -> Option<OsString>) -> Self {
        let env = |var: &str| env(var).filter(|v| !v.is_empty()).map(PathBuf::from);
        // systemd's LogsDirectory= (see etc/belower.service) wins for logs and
        // the store, for any user.
        let logs_directory = env("LOGS_DIRECTORY");
        let user_dir = |xdg_var: &str, under_home: &str| {
            env(xdg_var)
                .or_else(|| env("HOME").map(|home| home.join(under_home)))
                .map(|dir| dir.join("belower"))
        };
        let (config_dir, state_dir) = if is_root {
            (None, None)
        } else {
            (
                user_dir("XDG_CONFIG_HOME", ".config"),
                user_dir("XDG_STATE_HOME", ".local/state"),
            )
        };

        Self {
            config: config_dir.map_or(SYSTEM_CONF.into(), |dir| dir.join("belower.conf")),
            log_dir: logs_directory
                .clone()
                .or_else(|| state_dir.clone())
                .unwrap_or_else(std::env::temp_dir),
            store_dir: logs_directory
                .map(|dir| dir.join("store"))
                .or_else(|| state_dir.map(|dir| dir.join("store")))
                .unwrap_or_else(|| SYSTEM_STORE.into()),
        }
    }
}

/// Whether `dir` is a store holding at least one recording.
pub fn has_recordings(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().starts_with("index_"))
    })
}

/// Fail with advice on how to start recording if `dir` holds no recordings.
pub fn ensure_recordings(dir: &Path) -> Result<()> {
    if !has_recordings(dir) {
        bail!(
            "No recordings found in {}.\n\
            To start recording, run `belower record`, or set up the system-wide \
            recorder with `sudo systemctl enable --now belower`.",
            dir.display()
        );
    }
    Ok(())
}

/// Global below config
pub static BELOW_CONFIG: OnceLock<BelowConfig> = OnceLock::new();

#[derive(Serialize, Deserialize, Debug)]
// If value is missing during deserialization, use the Default::default()
#[serde(default)]
pub struct BelowConfig {
    pub log_dir: PathBuf,
    pub store_dir: PathBuf,
    pub cgroup_root: PathBuf,
    pub cgroup_filter_out: String,
    pub enable_gpu_stats: bool,
    pub use_rgpu_for_gpu_stats: bool,
    pub enable_btrfs_stats: bool,
    pub btrfs_samples: u64,
    pub btrfs_min_pct: f64,
    pub enable_ethtool_stats: bool,
    pub enable_ksm_stats: bool,
    pub enable_resctrl_stats: bool,
    pub enable_tc_stats: bool,
    /// Source cgroup cpu/memory stats from BPF (one flush at the cgroup root
    /// plus an in-kernel tree traversal) instead of reading each cgroupfs file.
    /// Requires a kernel with cgroup-iter support (>= 6.1) and CAP_BPF; falls
    /// back to file reads transparently when unavailable.
    pub enable_cgroup_bpf: bool,
    pub process_stack_trace_filter: ProcessStackTraceFilter,
    pub mlock_record: bool,
}

impl Default for BelowConfig {
    fn default() -> Self {
        let Locations {
            log_dir, store_dir, ..
        } = Locations::current();
        BelowConfig {
            log_dir,
            store_dir,
            cgroup_root: cgroupfs::DEFAULT_CG_ROOT.into(),
            cgroup_filter_out: String::new(),
            enable_gpu_stats: false,
            use_rgpu_for_gpu_stats: true,
            enable_btrfs_stats: false,
            btrfs_samples: btrfs::DEFAULT_SAMPLES,
            btrfs_min_pct: btrfs::DEFAULT_MIN_PCT,
            enable_ethtool_stats: false,
            enable_ksm_stats: false,
            enable_resctrl_stats: false,
            enable_tc_stats: false,
            enable_cgroup_bpf: false,
            process_stack_trace_filter: Default::default(),
            mlock_record: false,
        }
    }
}

impl BelowConfig {
    pub fn load(path: &Path) -> Result<Self> {
        match path.exists() {
            true if !path.is_file() => bail!("{} exists and is not a file", path.to_string_lossy()),
            true => BelowConfig::load_exists(path),
            false if path == Locations::current().config => Ok(Default::default()),
            false => bail!("No such file or directory: {}", path.to_string_lossy()),
        }
    }

    /// Read recordings from the system-wide store instead of this user's, if
    /// the store is the per-user default, holds no recordings, and the
    /// system-wide one has some. Returns whether it switched.
    pub fn fall_back_to_system_store(&mut self) -> bool {
        let system_store = Path::new(SYSTEM_STORE);
        let switch = self.store_dir != system_store
            && self.store_dir == Locations::current().store_dir
            && !has_recordings(&self.store_dir)
            && has_recordings(system_store);
        if switch {
            self.store_dir = system_store.into();
        }
        switch
    }

    fn load_exists(path: &Path) -> Result<Self> {
        let string_config = match fs::read_to_string(path) {
            Ok(sc) => sc,
            Err(e) => {
                bail!(
                    "Failed to read from config file {}: {}",
                    path.to_string_lossy(),
                    e
                );
            }
        };

        match toml::from_str(string_config.as_str()) {
            Ok(bc) => Ok(bc),
            Err(e) => {
                bail!(
                    "Failed to parse config file {}: {}",
                    path.to_string_lossy(),
                    e.message()
                );
            }
        }
    }
}
