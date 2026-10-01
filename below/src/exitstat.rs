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

use std::ffi::CStr;
use std::io;
use std::os::fd::AsFd;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use aya::EbpfLoader;
use aya::maps::PerfEventArray;
use aya::maps::perf::PerfEvent;
use aya::programs::TracePoint;
use below_exitstat_common::ABSENT;
use below_exitstat_common::Event;
use below_exitstat_common::Offsets;
use btf_rs::Btf;
use btf_rs::Type;
use fallible_iterator::FallibleIterator;
use nix::poll::PollFd;
use nix::poll::PollFlags;
use nix::poll::PollTimeout;
use slog::debug;
use slog::warn;

/// The exitstat BPF program, built from exitstat-ebpf by build.rs.
static EXITSTAT_BPF: &[u8] =
    aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/exitstat.bpf.o"));

const KERNEL_BTF: &str = "/sys/kernel/btf/vmlinux";

/// Pages per CPU in the perf buffer, as libbpf's default.
const PERF_BUFFER_PAGES: usize = 64;

static PAGE_SIZE: LazyLock<u64> = LazyLock::new(page_size);

fn page_size() -> u64 {
    match unsafe { libc::sysconf(libc::_SC_PAGESIZE) } {
        -1 => panic!("Failed to query page size"),
        x => x as u64,
    }
}

/// Struct layout lookups against kernel BTF.
struct KernelTypes(Btf);

impl KernelTypes {
    /// Follow typedefs and qualifiers to the underlying type.
    fn strip(&self, mut ty: Type) -> Result<Type> {
        loop {
            ty = match &ty {
                Type::Typedef(t) => self.0.resolve_chained_type(t),
                Type::Const(t) => self.0.resolve_chained_type(t),
                Type::Volatile(t) => self.0.resolve_chained_type(t),
                Type::Restrict(t) => self.0.resolve_chained_type(t),
                Type::TypeTag(t) => self.0.resolve_chained_type(t),
                _ => return Ok(ty),
            }?;
        }
    }

    fn size_of(&self, ty: &Type) -> Result<u64> {
        Ok(match self.strip(ty.clone())? {
            Type::Int(t) => t.size() as u64,
            Type::Ptr(_) => size_of::<u64>() as u64,
            Type::Struct(t) | Type::Union(t) => t.size() as u64,
            Type::Enum(t) => t.size() as u64,
            Type::Enum64(t) => t.size() as u64,
            Type::Array(t) => t.len() as u64 * self.size_of(&self.0.resolve_chained_type(&t)?)?,
            other => bail!("Cannot size a BTF {}", other.name()),
        })
    }

    fn named_struct(&self, name: &str) -> Result<Type> {
        self.0
            .resolve_types_by_name(name)?
            .into_iter()
            .find(|t| matches!(t, Type::Struct(s) if !s.members.is_empty()))
            .ok_or_else(|| anyhow!("struct {name} is not in the kernel BTF"))
    }

    /// Find `field` in `ty`, looking through anonymous struct and union members
    /// as C does. Returns its byte offset and type, or None if there is none
    /// (including when `ty` is not a struct or union at all).
    fn member(&self, ty: &Type, field: &str) -> Result<Option<(u64, Type)>> {
        let (Type::Struct(s) | Type::Union(s)) = self.strip(ty.clone())? else {
            return Ok(None);
        };
        for m in &s.members {
            let name = self.0.resolve_name(m).unwrap_or_default();
            let member_type = self.0.resolve_chained_type(m)?;
            if name == field {
                if m.bitfield_size().unwrap_or(0) != 0 {
                    bail!("{field} is a bitfield");
                }
                return Ok(Some((m.bit_offset() as u64 / 8, member_type)));
            }
            if name.is_empty() {
                if let Some((offset, t)) = self.member(&member_type, field)? {
                    return Ok(Some((m.bit_offset() as u64 / 8 + offset, t)));
                }
            }
        }
        Ok(None)
    }

    /// Byte offset of a C member path such as `ioac.read_bytes` or
    /// `rss_stat[1].count` within `struct root`, or None if any part of the path
    /// does not exist on this kernel.
    fn offset_of(&self, root: &str, path: &str) -> Result<Option<u64>> {
        let mut ty = self.named_struct(root)?;
        let mut offset = 0;
        for part in path.split('.') {
            let (name, index) = match part.split_once('[') {
                Some((name, index)) => (name, Some(index.trim_end_matches(']').parse::<u64>()?)),
                None => (part, None),
            };
            let Some((member_offset, member_type)) = self.member(&ty, name)? else {
                return Ok(None);
            };
            offset += member_offset;
            ty = member_type;
            if let Some(index) = index {
                let Type::Array(array) = self.strip(ty)? else {
                    bail!("{root}.{path}: {name} is not an array");
                };
                let element = self.0.resolve_chained_type(&array)?;
                offset += index * self.size_of(&element)?;
                ty = element;
            }
        }
        Ok(Some(offset))
    }

    fn required(&self, root: &str, path: &str) -> Result<u64> {
        self.offset_of(root, path)?
            .ok_or_else(|| anyhow!("{root}.{path} is not in the kernel BTF"))
    }

    fn optional(&self, root: &str, path: &str) -> Result<u64> {
        Ok(self.offset_of(root, path)?.unwrap_or(ABSENT))
    }

    /// Value of an enumerator, searching every enum: the MM_* counters are in
    /// an anonymous one.
    fn enumerator(&self, item: &str) -> Result<u64> {
        let mut types = self.0.type_iter();
        while let Some(ty) = types.next()? {
            if let Type::Enum(e) = ty {
                for m in &e.members {
                    if self.0.resolve_name(m)? == item {
                        return Ok(m.val() as u64);
                    }
                }
            }
        }
        bail!("enumerator {item} is not in the kernel BTF")
    }

    /// Everything the exitstat program reads, laid out for this kernel.
    fn exitstat_offsets(&self) -> Result<Offsets> {
        // task_cpu(): its own field before 5.16, in thread_info after.
        let cpu = match self.offset_of("task_struct", "cpu")? {
            Some(offset) => offset,
            None => self.required("task_struct", "thread_info.cpu")?,
        };

        // Before 6.2 rss_stat is a struct of atomic_long_t counters; from 6.2 on
        // it is an array of percpu_counter.
        let pre_6_2 = self.offset_of("mm_struct", "rss_stat.count")?.is_some();
        let mut rss = [ABSENT; 3];
        for (slot, counter) in rss
            .iter_mut()
            .zip(["MM_FILEPAGES", "MM_ANONPAGES", "MM_SHMEMPAGES"])
        {
            let i = self.enumerator(counter)?;
            *slot = if pre_6_2 {
                self.required("mm_struct", &format!("rss_stat.count[{i}].counter"))?
            } else {
                self.required("mm_struct", &format!("rss_stat[{i}].count"))?
            };
        }

        Ok(Offsets {
            real_parent: self.required("task_struct", "real_parent")?,
            group_leader: self.required("task_struct", "group_leader")?,
            sessionid: self.optional("task_struct", "sessionid")?,
            cpu,
            min_flt: self.required("task_struct", "min_flt")?,
            maj_flt: self.required("task_struct", "maj_flt")?,
            utime: self.required("task_struct", "utime")?,
            stime: self.required("task_struct", "stime")?,
            signal: self.required("task_struct", "signal")?,
            read_bytes: self.optional("task_struct", "ioac.read_bytes")?,
            write_bytes: self.optional("task_struct", "ioac.write_bytes")?,
            start_time: self.required("task_struct", "start_time")?,
            mm: self.required("task_struct", "mm")?,
            tgid: self.required("task_struct", "tgid")?,
            nr_threads: self.required("signal_struct", "nr_threads")?,
            rss,
            rss_clamp: (!pre_6_2).into(),
        })
    }
}

/// Resolve the exitstat program's offsets for the running kernel.
fn kernel_offsets() -> Result<Offsets> {
    let btf = Btf::from_file(KERNEL_BTF)
        .with_context(|| format!("Failed to read kernel BTF from {KERNEL_BTF}"))?;
    KernelTypes(btf)
        .exitstat_offsets()
        .context("Failed to find the task fields exitstat reads in the kernel BTF")
}

/// Whether `e` failed for lack of privileges (no CAP_BPF, CAP_PERFMON, ...).
pub fn is_permission_denied(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|e| e.kind() == io::ErrorKind::PermissionDenied)
    })
}

pub struct ExitstatDriver {
    logger: slog::Logger,
    debug: bool,
    buffer: Arc<Mutex<procfs::PidMap>>,
}

impl ExitstatDriver {
    pub fn new(logger: slog::Logger, debug: bool) -> Self {
        Self {
            logger,
            debug,
            buffer: Arc::new(Mutex::new(procfs::PidMap::default())),
        }
    }

    pub fn get_buffer(&self) -> Arc<Mutex<procfs::PidMap>> {
        self.buffer.clone()
    }

    fn handle_event(handle: &Arc<Mutex<procfs::PidMap>>, event: &Event) {
        // The ffi::CStr constructors don't like interior nuls
        let mut comm_no_interior_nul = Vec::with_capacity(16);
        for b in &event.comm {
            if *b != 0 {
                comm_no_interior_nul.push(*b);
            } else {
                break;
            }
        }
        comm_no_interior_nul.push(0);

        let pidinfo = procfs::PidInfo {
            stat: procfs::PidStat {
                pid: Some(event.tid),
                comm: CStr::from_bytes_with_nul(&comm_no_interior_nul).map_or_else(
                    |_| None,
                    |v| v.to_str().map_or_else(|_| None, |v| Some(v.to_string())),
                ),
                state: Some(procfs::PidState::Dead),
                ppid: Some(event.ppid),
                pgrp: Some(event.pgrp),
                session: Some(event.sid),
                minflt: Some(event.min_flt),
                majflt: Some(event.maj_flt),
                user_usecs: Some(event.utime_us),
                system_usecs: Some(event.stime_us),
                num_threads: Some(event.nr_threads),
                running_secs: Some(event.etime_us / 1000000),
                rss_bytes: Some(event.active_rss_pages * *PAGE_SIZE),
                processor: Some(event.cpu),
            },
            io: procfs::PidIo {
                rbytes: Some(event.io_read_bytes),
                wbytes: Some(event.io_write_bytes),
            },
            // It seems to be somewhat tricky to get a cgroup name using bpf. It might be possible
            // with the bpf_get_current_cgroup_id() helper, but that returns what looks like an
            // inode number. I'm not sure if it's easy/possible to translate an inode # to a path.
            cgroup: "?".to_string(),
            // We can't access cmdline b/c it requires taking mmap_sem and a
            // bunch of memory management helpers.
            ..Default::default()
        };

        // handle.lock() only fails if a thread holding the lock panic'd, in which
        // case we should probably panic too.
        handle
            .lock()
            .expect("lock poisoned: a thread holding the lock panicked")
            .insert(event.tid, pidinfo);
    }

    fn handle_lost_events(logger: &slog::Logger, cpu: u32, count: u64) {
        warn!(logger, "Lost {} events on CPU {}", count, cpu);
    }

    /// Loops forever unless an error is hit
    pub fn drive(&mut self) -> Result<()> {
        let offsets = kernel_offsets()?;
        if self.debug {
            debug!(self.logger, "exitstat kernel offsets: {:?}", offsets);
        }

        let mut bpf = EbpfLoader::new()
            .override_global("EXITSTAT_OFFSETS", &offsets, true)
            .load(EXITSTAT_BPF)
            .context("Failed to open BPF program")?;
        let program: &mut TracePoint = bpf
            .program_mut("exitstat")
            .context("BPF object has no exitstat program")?
            .try_into()?;
        program.load().context("Failed to load BPF program")?;
        program
            .attach("sched", "sched_process_exit")
            .context("Failed to attach BPF program?")?;

        // Set up one perf buffer per CPU
        let mut events = PerfEventArray::try_from(
            bpf.take_map("EVENTS")
                .context("BPF object has no EVENTS map")?,
        )?;
        let cpus = aya::util::online_cpus()
            .map_err(|(path, e)| anyhow!(e).context(format!("Failed to read {path}")))?;
        let mut buffers = cpus
            .iter()
            .map(|&cpu| Ok((cpu, events.open(cpu, Some(PERF_BUFFER_PAGES))?)))
            .collect::<Result<Vec<_>>>()
            .context("Failed to open perf buffers")?;

        // Poll events
        let buffer = self.get_buffer();
        loop {
            let mut fds: Vec<PollFd> = buffers
                .iter()
                .map(|(_, b)| PollFd::new(b.as_fd(), PollFlags::POLLIN))
                .collect();
            match nix::poll::poll(&mut fds, PollTimeout::from(100u16)) {
                Ok(_) | Err(nix::errno::Errno::EINTR) => {}
                Err(e) => return Err(e).context("Error polling perf buffer"),
            }
            drop(fds);
            for (cpu, perf) in &mut buffers {
                perf.for_each(|e| match e {
                    PerfEvent::Sample { head, tail } => match read_event(head, tail) {
                        Some(event) => Self::handle_event(&buffer, &event),
                        None => warn!(self.logger, "Short exitstat event on CPU {}", cpu),
                    },
                    PerfEvent::Lost { count } => {
                        Self::handle_lost_events(&self.logger, *cpu, count)
                    }
                });
            }
        }
    }
}

/// Decode one sample, which the perf ring may have split in two at its end.
fn read_event(head: &[u8], tail: &[u8]) -> Option<Event> {
    const LEN: usize = size_of::<Event>();
    if head.len() + tail.len() < LEN {
        return None;
    }
    let mut bytes = [0u8; LEN];
    let from_head = head.len().min(LEN);
    bytes[..from_head].copy_from_slice(&head[..from_head]);
    bytes[from_head..].copy_from_slice(&tail[..LEN - from_head]);
    // SAFETY: Event is plain integers, so any bytes are a valid value.
    Some(unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_event_joins_wrapped_sample() {
        let event = Event {
            tid: 7,
            comm: *b"abc\0\0\0\0\0\0\0\0\0\0\0\0\0",
            active_rss_pages: 42,
            ..Default::default()
        };
        // SAFETY: Event is plain integers.
        let bytes: [u8; size_of::<Event>()] = unsafe { std::mem::transmute(event) };
        for split in [0, 1, 40, size_of::<Event>()] {
            let (head, tail) = bytes.split_at(split);
            assert_eq!(read_event(head, tail), Some(event));
        }
        assert_eq!(read_event(&bytes[..100], &[]), None);
    }

    /// Resolves against the host kernel's BTF, where it has one.
    #[test]
    fn kernel_offsets_resolve() {
        if !std::path::Path::new(KERNEL_BTF).exists() {
            return;
        }
        let offsets = kernel_offsets().unwrap();
        assert_ne!(offsets.cpu, ABSENT);
        assert_ne!(offsets.tgid, offsets.real_parent);
        assert!(offsets.rss.iter().all(|&o| o != ABSENT));
        assert!(offsets.rss[0] < offsets.rss[1] && offsets.rss[1] < offsets.rss[2]);
    }
}
