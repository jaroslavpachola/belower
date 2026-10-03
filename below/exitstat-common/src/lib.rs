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

//! Types shared by the exitstat BPF program (below-exitstat-ebpf, built for the
//! BPF target) and the loader in below. Both sides compile this one definition,
//! so the layout of the offsets the loader writes and the events the program
//! emits cannot drift apart.

#![no_std]

/// Marks a field the running kernel does not have. The program reads 0 for it.
pub const ABSENT: u64 = u64::MAX;

/// Where the fields exitstat reads live in the running kernel's structs.
///
/// The BPF program is built without CO-RE relocations, so it cannot learn
/// these at load time on its own. below resolves each one from the kernel's
/// BTF (/sys/kernel/btf/vmlinux) and writes them into the program's read-only
/// data before loading it. A path through an embedded struct, such as
/// `task->ioac.read_bytes`, is summed into one offset from the pointer it
/// starts at.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offsets {
    // From `struct task_struct`.
    pub real_parent: u64,
    pub group_leader: u64,
    /// `ABSENT` without CONFIG_AUDIT.
    pub sessionid: u64,
    /// `task->cpu` before 5.16, `task->thread_info.cpu` after.
    pub cpu: u64,
    pub min_flt: u64,
    pub maj_flt: u64,
    pub utime: u64,
    pub stime: u64,
    pub signal: u64,
    /// `task->ioac.read_bytes`; `ABSENT` without CONFIG_TASK_IO_ACCOUNTING.
    pub read_bytes: u64,
    /// `task->ioac.write_bytes`; `ABSENT` without CONFIG_TASK_IO_ACCOUNTING.
    pub write_bytes: u64,
    pub start_time: u64,
    pub mm: u64,
    pub tgid: u64,
    // From `struct signal_struct`.
    pub nr_threads: u64,
    /// From `struct mm_struct`: the MM_FILEPAGES, MM_ANONPAGES and
    /// MM_SHMEMPAGES counters. Before 6.2 these are
    /// `rss_stat.count[i].counter`; from 6.2 on they are `rss_stat[i].count`.
    pub rss: [u64; 3],
    /// Non-zero from 6.2 on, where the counters are `struct percpu_counter`
    /// and a negative value is read as 0 (as `percpu_counter_read_positive`
    /// does).
    pub rss_clamp: u64,
}

impl Offsets {
    /// Every field absent: what the program reads if below never fills them in.
    pub const UNRESOLVED: Self = Self {
        real_parent: ABSENT,
        group_leader: ABSENT,
        sessionid: ABSENT,
        cpu: ABSENT,
        min_flt: ABSENT,
        maj_flt: ABSENT,
        utime: ABSENT,
        stime: ABSENT,
        signal: ABSENT,
        read_bytes: ABSENT,
        write_bytes: ABSENT,
        start_time: ABSENT,
        mm: ABSENT,
        tgid: ABSENT,
        nr_threads: ABSENT,
        rss: [ABSENT; 3],
        rss_clamp: 0,
    };
}

/// One task exit, as the program sends it through the `EVENTS` perf buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Event {
    /// Thread (task) ID.
    pub tid: i32,
    /// Parent process ID.
    pub ppid: i32,
    /// tgid of the thread group leader.
    pub pgrp: i32,
    /// Audit session ID.
    pub sid: i32,
    /// CPU the task last ran on.
    pub cpu: i32,
    /// Task name, NUL-padded.
    pub comm: [u8; 16],
    /// Explicit so every byte the program sends is initialized.
    pub _pad: u32,
    /// Minor page faults.
    pub min_flt: u64,
    /// Major page faults.
    pub maj_flt: u64,
    pub utime_us: u64,
    pub stime_us: u64,
    /// Time since the task started.
    pub etime_us: u64,
    /// Threads in the task's thread group.
    pub nr_threads: u64,
    /// Bytes this task caused to be read from storage.
    pub io_read_bytes: u64,
    /// Bytes this task caused to be written to storage.
    pub io_write_bytes: u64,
    /// File, anon and shmem pages mapped at exit. Always 0 on older kernels
    /// (seen on 6.12 and earlier), which release the mm before the
    /// sched_process_exit tracepoint fires.
    pub active_rss_pages: u64,
}

#[cfg(feature = "user")]
// SAFETY: plain #[repr(C)] integers with no implicit padding.
unsafe impl aya::Pod for Offsets {}

#[cfg(feature = "user")]
// SAFETY: plain #[repr(C)] integers; the only padding is the explicit `_pad`.
unsafe impl aya::Pod for Event {}

// The program and loader agree by construction; this pins the wire size too.
const _: () = assert!(core::mem::size_of::<Event>() == 112);
