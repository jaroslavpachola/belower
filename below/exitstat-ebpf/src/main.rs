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

//! below's exitstat BPF program: on every task exit, send the task's final
//! accounting to user space.
//!
//! Rust BPF programs get no CO-RE relocations, so every kernel struct field is
//! read at an offset that below computes from the running kernel's BTF and writes
//! into `EXITSTAT_OFFSETS` before loading (see below/src/exitstat.rs).

#![no_std]
#![no_main]

use aya_ebpf::helpers::bpf_get_current_comm;
use aya_ebpf::helpers::bpf_get_current_pid_tgid;
use aya_ebpf::helpers::bpf_get_current_task;
use aya_ebpf::helpers::bpf_ktime_get_ns;
use aya_ebpf::helpers::bpf_probe_read_kernel;
use aya_ebpf::macros::map;
use aya_ebpf::macros::tracepoint;
use aya_ebpf::maps::PerfEventArray;
use aya_ebpf::programs::TracePointContext;
use below_exitstat_common::ABSENT;
use below_exitstat_common::Event;
use below_exitstat_common::Offsets;

/// Filled in by below before load; read-only to the program.
#[unsafe(no_mangle)]
static EXITSTAT_OFFSETS: Offsets = Offsets::UNRESOLVED;

#[map]
static EVENTS: PerfEventArray<Event> = PerfEventArray::new(0);

/// Read one offset. Volatile, so the compiler cannot fold in the placeholder
/// value the object is built with.
macro_rules! offset {
    ($($field:tt)+) => {
        unsafe { core::ptr::read_volatile(&raw const EXITSTAT_OFFSETS.$($field)+) }
    };
}

/// Read a `T` at `base + offset` in kernel memory, or 0 if the pointer is NULL,
/// the field is absent on this kernel, or the read faults.
#[inline(always)]
fn read<T: Default>(base: u64, offset: u64) -> T {
    if base == 0 || offset == ABSENT {
        return T::default();
    }
    unsafe { bpf_probe_read_kernel((base + offset) as *const T) }.unwrap_or_default()
}

/// sched:sched_process_exit fires as a task exits, so this sees the final
/// accounting of tasks that live less than one sample interval.
#[tracepoint]
pub fn exitstat(ctx: TracePointContext) -> u32 {
    let task = unsafe { bpf_get_current_task() };
    let now = unsafe { bpf_ktime_get_ns() };
    let tgid = offset!(tgid);

    let parent: u64 = read(task, offset!(real_parent));
    let leader: u64 = read(task, offset!(group_leader));
    let signal: u64 = read(task, offset!(signal));
    let mm: u64 = read(task, offset!(mm));

    let clamp = offset!(rss_clamp) != 0;
    let mut rss_pages = 0u64;
    for i in 0..3 {
        let pages: i64 = read(mm, offset!(rss[i]));
        rss_pages = rss_pages.wrapping_add(if clamp && pages < 0 { 0 } else { pages as u64 });
    }

    let start_time: u64 = read(task, offset!(start_time));
    let event = Event {
        tid: bpf_get_current_pid_tgid() as u32 as i32,
        ppid: read(parent, tgid),
        pgrp: read(leader, tgid),
        sid: read(task, offset!(sessionid)),
        cpu: read(task, offset!(cpu)),
        comm: bpf_get_current_comm().unwrap_or_default(),
        _pad: 0,
        min_flt: read(task, offset!(min_flt)),
        maj_flt: read(task, offset!(maj_flt)),
        utime_us: read::<u64>(task, offset!(utime)) / 1000,
        stime_us: read::<u64>(task, offset!(stime)) / 1000,
        etime_us: now.wrapping_sub(start_time) / 1000,
        nr_threads: read::<i32>(signal, offset!(nr_threads)) as u64,
        io_read_bytes: read(task, offset!(read_bytes)),
        io_write_bytes: read(task, offset!(write_bytes)),
        active_rss_pages: rss_pages,
    };
    EVENTS.output(&ctx, &event, 0);
    0
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
#[unsafe(link_section = "license")]
pub static LICENSE: [u8; 4] = *b"GPL\0";
