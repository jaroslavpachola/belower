# Belower Configuration

belower reads an optional `toml` configuration file that defines, among other things, its `log` and `store` paths. You can always override the configuration path with the `--config` CLI argument.

## Default locations

Run as root (for example by the systemd service), belower uses system-wide locations. Run as any other user, it uses per-user ones, so recording and replaying work without root:

| | As root | As another user |
|---|---|---|
| Config file | `/etc/belower/belower.conf` | `~/.config/belower/belower.conf` (`$XDG_CONFIG_HOME/belower/`) |
| Log directory | the system temp directory | `~/.local/state/belower` (`$XDG_STATE_HOME/belower/`) |
| Store | `/var/log/belower/store` | `~/.local/state/belower/store` |

Under systemd, `LogsDirectory=` (`$LOGS_DIRECTORY`) sets the log directory and puts the store in its `store` subdirectory, for any user.

When a user without recordings of their own runs `replay`, `dump` or `snapshot`, belower reads the system-wide store in `/var/log/belower/store` instead, if it has recordings.

## Example
```
# /etc/belower/belower.conf

log_dir = "/var/log/belower"
store_dir = "/var/log/belower/store"
cgroup_filter_out = "user.slice.*"
cgroup_root = "/sys/fs/cgroup/unified"
```

## Attributes
* `log_dir` -- Takes a string path and uses as the logging directory. See the default locations above.
* `store_dir` -- Takes a string path and uses as the store directory. See the default locations above.
* `cgroup_filter_out` -- Takes a regex string and belower will no longer collect cgroup data if cgroup full path match the regex.
* `cgroup_root` -- Path to cgroup2 mountpoint, defaults to `/sys/fs/cgroup`.

## To override the default value
1. Edit the config file (`/etc/belower/belower.conf` for the service) with desired value.
2. Restart belower service.

## Notes
* After changing the `store_dir`, `belower replay` may fail because of missing store directory. You can copy the old store folder to the updated location if you need historical data or simply restart the belower service if you don't.
* If the default configuration file is missing, `belower` will use the default value. But if you override the config with a non-existing path, `belower` will raise an error.
* Any unset option in the config file will be implicitly set to the default value
