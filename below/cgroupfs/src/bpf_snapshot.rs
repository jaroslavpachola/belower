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

//! The stats the BPF cgroup reader hands the collector, and the handle it hands
//! them over through. These exist with or without the `cgroup-bpf` feature, so
//! callers need no feature gates of their own: without it, `start_cgroup_bpf`
//! fails and they read the cgroup files.

use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;

use anyhow::Result;

use crate::CgroupStat;
use crate::CpuStat;
use crate::MemoryEvents;
use crate::MemoryEventsLocal;
use crate::MemoryStat;

/// Per-cgroup stats sourced from BPF and overlaid onto file reads. Every
/// `Option` field falls back to the cgroupfs file when `None` (the kernel could
/// not supply it, or the cgroup was absent from the BPF traversal), so the data
/// is never partial or wrong. `parent_id` is BPF-only metadata with no file
/// equivalent -- the parent cgroup's id, 0 for the root -- so the tree can be
/// rebuilt from a snapshot alone.
#[derive(Clone, Debug, Default)]
pub struct CgroupBpfStat {
    pub parent_id: u64,
    pub cpu_stat: Option<CpuStat>,
    pub memory_current: Option<i64>,
    pub memory_stat: Option<MemoryStat>,
    pub cgroup_stat: Option<CgroupStat>,
    pub memory_events: Option<MemoryEvents>,
    pub memory_events_local: Option<MemoryEventsLocal>,
    pub memory_min: Option<i64>,
    pub memory_low: Option<i64>,
    pub memory_high: Option<i64>,
    pub memory_max: Option<i64>,
    pub memory_oom_group: Option<u32>,
}

/// A sample's BPF-collected cgroup stats, keyed by cgroup id -- which equals the
/// cgroup directory inode `CgroupReader::read_inode_number` returns, so it joins
/// directly against the cgroupfs walk.
pub type CgroupBpfSnapshot = HashMap<u64, CgroupBpfStat>;

/// Handle for obtaining a fresh BPF snapshot each sample. It carries only
/// channels, so the collector can request a snapshot without touching libbpf.
/// `collect` sends a request and blocks for the reply; the exchange is 1:1 and
/// in order, so the snapshot always matches this call.
pub struct CgroupBpfHandle {
    req: Sender<()>,
    resp: Receiver<Result<CgroupBpfSnapshot>>,
}

impl CgroupBpfHandle {
    pub fn new(req: Sender<()>, resp: Receiver<Result<CgroupBpfSnapshot>>) -> Self {
        Self { req, resp }
    }

    /// Ask the driver to flush at the cgroup root, traverse the tree, and reply
    /// with the fresh snapshot; block until it arrives. Returns `None` on any
    /// error or if the driver is not running, so the caller falls back to files.
    /// Setup errors surface via the driver's own error channel, so they are
    /// swallowed here to avoid per-sample log spam.
    pub fn collect(&self) -> Option<CgroupBpfSnapshot> {
        if self.req.send(()).is_err() {
            return None;
        }
        match self.resp.recv() {
            Ok(Ok(snapshot)) => Some(snapshot),
            Ok(Err(_)) => None,
            Err(_) => None,
        }
    }
}

/// Stand-in for the BPF cgroup reader in builds without the `cgroup-bpf`
/// feature, so callers handle it like any other failure to start.
#[cfg(not(feature = "cgroup-bpf"))]
pub fn start_cgroup_bpf(
    _debug: bool,
    _cgroup_root: std::path::PathBuf,
) -> Result<Option<CgroupBpfHandle>> {
    anyhow::bail!("below was built without the cgroup-bpf feature")
}
