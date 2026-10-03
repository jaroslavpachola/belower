# belower

<div align="center">
  <p>
    <a href="https://github.com/jaroslavpachola/belower/actions/workflows/ci.yml">
      <img alt="CI" src="https://github.com/jaroslavpachola/belower/actions/workflows/ci.yml/badge.svg" />
    </a>
  </p>
</div>

**A C-free, more ergonomic [below](https://github.com/facebookincubator/below).**

`belower` is an interactive tool to view and record historical system data,
forked from Meta's `below`. It has support for:

* information regarding hardware resource utilization
* viewing the cgroup hierarchy
* cgroup and process information
* pressure stall information (PSI)
* `record` mode to record system data
* `replay` mode to replay historical system data
* `live` mode to view live system data
* `dump` subcommand to report script-friendly information (eg JSON, CSV, OpenMetrics, etc.)
* `snapshot` subcommand to create a replayable snapshot file of historical system data

belower does **not** have support for cgroup1.

## Differences from below

belower is a standalone project and does not track upstream `below`. So far:

* **No C in the default build.** The exitstat BPF program, which records
  processes that exit between samples, is written in Rust and loaded with
  [aya](https://aya-rs.dev). Instead of CO-RE, belower reads the running
  kernel's BTF at startup and computes the field offsets the program needs.
  The build no longer needs clang, libelf, zlib or libbpf; zstd is the only C
  dependency left.
* **BPF cgroup stats are optional.** below's C cgroup BPF reader is built only
  with the `cgroup-bpf` feature. Without it, belower reads the cgroup files.
* **Lighter live view.** The view refreshes when a new sample arrives instead
  of rebuilding itself four times a second.
* **Works without root.** As a regular user, `record`, `replay` and `dump`
  use per-user paths under `~/.config` and `~/.local/state`, and read the
  system-wide recordings when you have none. `replay` starts at the latest
  recording and `dump` at one hour ago unless told otherwise.
* **Familiar keys.** `j`/`k` move the selection, as in less and vim; jumping
  through time in replay or pause moved from `j`/`J` to `]`/`[`.
* **Mouse and hints.** Click rows, tabs and column titles (to sort), and
  scroll with the wheel; the status bar shows the main keys for the current
  mode. Set `mouse = false` under `[view]` in belowerrc to turn the mouse off.
* **Faster first screen.** Live mode shows rates after one second rather than
  `?` for a whole interval.
* **Remembers the view.** The screen, tabs, sorting and column widths you
  leave belower with are restored the next time it starts.

## Demo

This recording shows below's UI, which belower shares:

<a href="https://asciinema.org/a/355506">
<img src="https://asciinema.org/a/355506.svg" width="500">
</a>

## Installing

belower is not packaged by any distribution yet. Distribution packages named
`below` install upstream below, not belower.

First, install the dependencies listed in [building.md](docs/building.md).
Then:

```shell
$ cargo install --git https://github.com/jaroslavpachola/belower belower
$ belower --help
```

When working from a checkout, always use release builds (`cargo build
--release`, `cargo run --release`). Debug builds are several times slower and
make the live view use noticeably more CPU.

To run belower in a container, see [docker.md](docs/docker.md).

## Quickstart

Live view of system:

```shell
$ sudo belower live
```

Record as your own user (no root needed; data goes to
`~/.local/state/belower/store`):

```shell
$ belower record
```

Keep recording in the background with systemd, either system-wide (recording
to `/var/log/belower/store`) or as your own user service:

```shell
$ sudo $(which belower) service install
$ belower service install --user
```

`--dry-run` shows the unit and commands without running them, and
`belower service uninstall` removes the service again.

Replay historical data, starting from the latest recording or from a given
time:

```shell
$ belower replay
$ belower replay -t "3m ago"
```

Dump the last hour as CSV:

```shell
$ belower dump system -O csv
```

See where your recordings are, how much space they use and what time they
cover:

```shell
$ belower store info
```

See [belower_config.md](docs/belower_config.md) for where belower keeps its
config, logs and recordings.

## Integration with Prometheus/Grafana

`belower` has basic support for Prometheus/Grafana through the `dump`
interface.

See [contrib/grafana/](contrib/grafana) for more details.

## Comparison with alternative tools

See [comparison.md](docs/comparison.md) for a feature comparison
with alternative tools.

## Contributing

See the [CONTRIBUTING](CONTRIBUTING.md) file for how to help out.

## Credits and license

belower is based on [below](https://github.com/facebookincubator/below) by
Meta Platforms, Inc. and its contributors. below's name stems from its
developers rejecting many of [atop](https://linux.die.net/man/1/atop)'s design
and style decisions; belower goes a little further down.

belower is licensed under the Apache License 2.0, like below. See the
[LICENSE](LICENSE) file, and [NOTICE](NOTICE) for attribution and how changes
from below are recorded.
