# Belower Configuration

belower is expecting a `toml` configuration file to define its `log` and `store` path. The default location of this configuration path is `/etc/belower/belower.conf`. You can always override the default configuration path with `--config` CLI argument.

## Example
```
# /etc/belower/belower.conf

log_dir = "/var/log/belower"
store_dir = "/var/log/belower/store"
cgroup_filter_out = "user.slice.*"
cgroup_root = "/sys/fs/cgroup/unified"
```

## Attributes
* `log_dir` -- Takes a string path and uses as the logging directory, default to `/var/log/belower`.
* `store_dir` -- Takes a string path and uses as the store directory, default to `/var/log/belower/store`.
* `cgroup_filter_out` -- Takes a regex string and belower will no longer collect cgroup data if cgroup full path match the regex.
* `cgroup_root` -- Path to cgroup2 mountpoint, defaults to `/sys/fs/cgroup`.

## To override the default value
1. Edit `/etc/belower/belower.conf` with desired value.
2. Restart belower service.

## Notes
* After changing the `store_dir`, `belower replay` may fail because of missing store directory. You can copy the old store folder to the updated location if you need historical data or simply restart the belower service if you don't.
* If the default configuration file is missing, `belower` will use the default value. But if you override the config with a non-existing path, `belower` will raise an error.
* Any unset option in the config file will be implicitly set to the default value
