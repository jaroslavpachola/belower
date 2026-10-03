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

use std::io::Write;

use tempfile::TempDir;

use super::*;

#[test]
fn test_config_default() {
    let below_config: BelowConfig = Default::default();
    let locations = Locations::current();
    assert_eq!(below_config.log_dir, locations.log_dir);
    assert_eq!(below_config.store_dir, locations.store_dir);
    assert_eq!(
        below_config.cgroup_root.to_string_lossy(),
        cgroupfs::DEFAULT_CG_ROOT
    );
    assert_eq!(below_config.cgroup_filter_out, String::new());
    assert!(!below_config.enable_gpu_stats);
    assert!(!below_config.enable_btrfs_stats);
}

fn resolve(is_root: bool, vars: &[(&str, &str)]) -> Locations {
    let vars: std::collections::HashMap<_, _> = vars.iter().copied().collect();
    Locations::resolve(is_root, |var| vars.get(var).map(OsString::from))
}

#[test]
fn test_locations() {
    let home = [("HOME", "/home/u")];
    assert_eq!(
        resolve(true, &home),
        Locations {
            config: SYSTEM_CONF.into(),
            log_dir: std::env::temp_dir(),
            store_dir: SYSTEM_STORE.into(),
        }
    );
    assert_eq!(
        resolve(false, &home),
        Locations {
            config: "/home/u/.config/belower/belower.conf".into(),
            log_dir: "/home/u/.local/state/belower".into(),
            store_dir: "/home/u/.local/state/belower/store".into(),
        }
    );
    // XDG variables override HOME; empty ones are ignored.
    let xdg = resolve(
        false,
        &[
            ("HOME", "/home/u"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_STATE_HOME", ""),
        ],
    );
    assert_eq!(xdg.config, Path::new("/cfg/belower/belower.conf"));
    assert_eq!(
        xdg.store_dir,
        Path::new("/home/u/.local/state/belower/store")
    );
    // systemd's LOGS_DIRECTORY wins for logs and the store.
    let systemd = resolve(true, &[("LOGS_DIRECTORY", "/var/log/belower")]);
    assert_eq!(systemd.log_dir, Path::new("/var/log/belower"));
    assert_eq!(systemd.store_dir, Path::new("/var/log/belower/store"));
    // No HOME at all: fall back to the system-wide locations.
    assert_eq!(resolve(false, &[]), resolve(true, &[]));
}

#[test]
fn test_has_recordings() {
    let tempdir = TempDir::with_prefix("below_config_store.").expect("Failed to create temp dir");
    assert!(!has_recordings(&tempdir.path().join("missing")));
    assert!(!has_recordings(tempdir.path()));
    assert!(ensure_recordings(tempdir.path()).is_err());
    std::fs::write(tempdir.path().join("index_01790985600"), b"").unwrap();
    assert!(has_recordings(tempdir.path()));
    assert!(ensure_recordings(tempdir.path()).is_ok());
}

#[test]
fn test_config_fs_failure() {
    let tempdir =
        TempDir::with_prefix("below_config_fs_failuer.").expect("Failed to create temp dir");
    let path = tempdir.path();
    match BelowConfig::load(path) {
        Ok(_) => panic!("belower should not load if the non existing path is not default path"),
        Err(e) => assert_eq!(
            format!("{}", e),
            format!("{} exists and is not a file", path.to_string_lossy())
        ),
    }

    let path = tempdir.path().join("belower.config");
    match BelowConfig::load(&path) {
        Ok(_) => panic!("belower should not load if the non existing path is not default path"),
        Err(e) => assert_eq!(
            format!("{}", e),
            format!("No such file or directory: {}", path.to_string_lossy())
        ),
    }
}

#[test]
fn test_config_load_success() {
    let tempdir = TempDir::with_prefix("below_config_load.").expect("Failed to create temp dir");
    let path = tempdir.path().join("belower.config");

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(true)
        .create(true)
        .open(&path)
        .expect("Fail to open belower.conf in tempdir");
    let config_str = r#"
        log_dir = '/var/log/below'
        store_dir = '/var/log/below'
        cgroup_root = '/sys/fs/cgroup'
        cgroup_filter_out = 'user.slice'
        # I'm a comment
        something_else = "demacia"
    "#;
    file.write_all(config_str.as_bytes())
        .expect("Faild to write temp conf file during testing ignore");
    file.flush().expect("Failed to flush during testing ignore");

    let below_config = match BelowConfig::load(&path) {
        Ok(b) => b,
        Err(e) => panic!("{:#}", e),
    };
    assert_eq!(below_config.log_dir.to_string_lossy(), "/var/log/below");
    assert_eq!(below_config.store_dir.to_string_lossy(), "/var/log/below");
    assert_eq!(below_config.cgroup_root.to_string_lossy(), "/sys/fs/cgroup");
    assert_eq!(below_config.cgroup_filter_out, "user.slice");
}

#[test]
fn test_config_load_failed() {
    let tempdir =
        TempDir::with_prefix("below_config_load_failed.").expect("Failed to create temp dir");
    let path = tempdir.path().join("belower.config");
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(true)
        .create(true)
        .open(&path)
        .expect("Fail to open belower.conf in tempdir");
    let config_str = r#"
        log_dir = '/var/log/below'
        store_dir = '/var/log/below'
        # I'm a comment
        something_else = "demacia"
        Some invalid string that is not a comment
    "#;
    file.write_all(config_str.as_bytes())
        .expect("Faild to write temp conf file during testing ignore");
    file.flush()
        .expect("Failed to flush during testing failure");

    match BelowConfig::load(&path) {
        Ok(_) => panic!("belower should not load since it is an invalid configuration file"),
        Err(e) => {
            let err_msg = format!("{}", e);
            assert!(err_msg.starts_with("Failed to parse config file"));
            // Ensure raw file contents are not leaked in the error message
            assert!(
                !err_msg.contains("demacia"),
                "Error message should not contain raw file contents"
            );
        }
    }
}

#[test]
fn test_config_partial_load() {
    let tempdir = TempDir::with_prefix("below_config_load.").expect("Failed to create temp dir");
    let path = tempdir.path().join("belower.config");

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(true)
        .create(true)
        .open(&path)
        .expect("Fail to open belower.conf in tempdir");
    let config_str = r#"
        log_dir = 'my magic string'
    "#;
    file.write_all(config_str.as_bytes())
        .expect("Faild to write temp conf file during testing ignore");
    file.flush().expect("Failed to flush during testing ignore");

    let below_config = match BelowConfig::load(&path) {
        Ok(b) => b,
        Err(e) => panic!("{:#}", e),
    };
    assert_eq!(below_config.log_dir.to_string_lossy(), "my magic string");
    assert_eq!(below_config.store_dir, Locations::current().store_dir);
}

#[test]
fn test_config_valid_stack_trace_filter_single() {
    let tempdir = TempDir::with_prefix("below_config_stack.").expect("Failed to create temp dir");
    let path = tempdir.path().join("belower.config");

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(true)
        .create(true)
        .open(&path)
        .expect("Fail to open belower.conf in tempdir");
    let config_str = r#"
        process_stack_trace_filter = "Uninterruptible"
    "#;
    file.write_all(config_str.as_bytes())
        .expect("Failed to write temp conf file");
    file.flush().expect("Failed to flush");

    let below_config = BelowConfig::load(&path).expect("Should load valid config");
    assert_eq!(
        below_config.process_stack_trace_filter,
        ProcessStackTraceFilter::Uninterruptible
    );
}

#[test]
fn test_config_valid_stack_trace_filter_multiple() {
    let tempdir = TempDir::with_prefix("below_config_stack.").expect("Failed to create temp dir");
    let path = tempdir.path().join("belower.config");

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(true)
        .create(true)
        .open(&path)
        .expect("Fail to open belower.conf in tempdir");
    let config_str = r#"
        enable_process_stack_traces = true
        process_stack_trace_filter = "UninterruptibleAndRunning"
    "#;
    file.write_all(config_str.as_bytes())
        .expect("Failed to write temp conf file");
    file.flush().expect("Failed to flush");

    let below_config = BelowConfig::load(&path).expect("Should load valid config");
    assert_eq!(
        below_config.process_stack_trace_filter,
        ProcessStackTraceFilter::UninterruptibleAndRunning
    );
}

#[test]
fn test_config_valid_stack_trace_filter_all() {
    let tempdir = TempDir::with_prefix("below_config_stack.").expect("Failed to create temp dir");
    let path = tempdir.path().join("belower.config");

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(true)
        .create(true)
        .open(&path)
        .expect("Fail to open belower.conf in tempdir");
    let config_str = r#"
        process_stack_trace_filter = "All"
    "#;
    file.write_all(config_str.as_bytes())
        .expect("Failed to write temp conf file");
    file.flush().expect("Failed to flush");

    let below_config = BelowConfig::load(&path).expect("Should load valid config");
    assert_eq!(
        below_config.process_stack_trace_filter,
        ProcessStackTraceFilter::All
    );
}

#[test]
fn test_config_valid_stack_trace_filter_none() {
    let tempdir = TempDir::with_prefix("below_config_stack.").expect("Failed to create temp dir");
    let path = tempdir.path().join("belower.config");

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(true)
        .create(true)
        .open(&path)
        .expect("Fail to open belower.conf in tempdir");
    let config_str = r#"
        process_stack_trace_filter = "None"
    "#;
    file.write_all(config_str.as_bytes())
        .expect("Failed to write temp conf file");
    file.flush().expect("Failed to flush");

    let below_config = BelowConfig::load(&path).expect("Should load valid config");
    assert_eq!(
        below_config.process_stack_trace_filter,
        ProcessStackTraceFilter::None
    );
}

#[test]
fn test_config_invalid_stack_trace_filter_typo() {
    let tempdir = TempDir::with_prefix("below_config_stack.").expect("Failed to create temp dir");
    let path = tempdir.path().join("belower.config");

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(true)
        .create(true)
        .open(&path)
        .expect("Fail to open belower.conf in tempdir");
    let config_str = r#"
        process_stack_trace_filter = "Untiterruptible"
    "#;
    file.write_all(config_str.as_bytes())
        .expect("Failed to write temp conf file");
    file.flush().expect("Failed to flush");

    match BelowConfig::load(&path) {
        Ok(_) => panic!("Should not load config with invalid filter value"),
        Err(e) => {
            let err_msg = format!("{}", e);
            assert!(err_msg.contains("unknown variant"));
            assert!(err_msg.contains("Untiterruptible"));
        }
    }
}

#[test]
fn test_config_invalid_stack_trace_filter_empty_value() {
    let tempdir = TempDir::with_prefix("below_config_stack.").expect("Failed to create temp dir");
    let path = tempdir.path().join("belower.config");

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(true)
        .create(true)
        .open(&path)
        .expect("Fail to open belower.conf in tempdir");
    let config_str = r#"
        process_stack_trace_filter = ""
    "#;
    file.write_all(config_str.as_bytes())
        .expect("Failed to write temp conf file");
    file.flush().expect("Failed to flush");

    match BelowConfig::load(&path) {
        Ok(_) => panic!("Should not load config with empty filter value"),
        Err(e) => {
            let err_msg = format!("{}", e);
            assert!(err_msg.contains("unknown variant"));
        }
    }
}
