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

//! `belower service`: install the recorder as a systemd service.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;

/// The system-wide unit, as shipped in etc/.
const SYSTEM_UNIT: &str = include_str!("../../etc/belower.service");
const UNIT_NAME: &str = "belower.service";

/// Which systemd instance the service belongs to.
#[derive(Clone, Copy)]
pub enum Scope {
    /// System-wide, recording to /var/log/belower (needs root).
    System,
    /// This user's service manager, recording to the per-user store.
    User,
}

impl Scope {
    fn unit_path(self) -> Result<PathBuf> {
        Ok(match self {
            Scope::System => PathBuf::from("/etc/systemd/system").join(UNIT_NAME),
            Scope::User => {
                let env = |var| std::env::var_os(var).filter(|v| !v.is_empty());
                env("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .or_else(|| env("HOME").map(|home| PathBuf::from(home).join(".config")))
                    .ok_or_else(|| anyhow!("Neither XDG_CONFIG_HOME nor HOME is set"))?
                    .join("systemd/user")
                    .join(UNIT_NAME)
            }
        })
    }

    fn systemctl_args(self) -> &'static [&'static str] {
        match self {
            Scope::System => &[],
            Scope::User => &["--user"],
        }
    }
}

/// The unit file for `scope`, running the belower binary at `exe`.
fn unit(scope: Scope, exe: &Path) -> String {
    let exe = exe.display().to_string();
    SYSTEM_UNIT
        .lines()
        // Leave out the license header.
        .skip_while(|line| line.is_empty() || line.starts_with('#'))
        .filter_map(|line| match scope {
            // A user service's LogsDirectory would move the store; without
            // it, belower uses the per-user default.
            Scope::User if line.starts_with("LogsDirectory=") => None,
            // A user service is wanted by the user's default target.
            Scope::User if line == "WantedBy=multi-user.target" => {
                Some("WantedBy=default.target".to_string())
            }
            _ => Some(line.replace("/usr/bin/belower", &exe)),
        })
        .map(|line| line + "\n")
        .collect()
}

fn systemctl(scope: Scope, args: &[&str], dry_run: bool) -> Result<()> {
    let all: Vec<&str> = scope.systemctl_args().iter().chain(args).copied().collect();
    println!("systemctl {}", all.join(" "));
    if dry_run {
        return Ok(());
    }
    let status = Command::new("systemctl")
        .args(&all)
        .status()
        .context("Failed to run systemctl")?;
    if !status.success() {
        bail!("systemctl {} failed ({})", all.join(" "), status);
    }
    Ok(())
}

pub fn install(scope: Scope, dry_run: bool) -> Result<()> {
    let exe = std::env::current_exe().context("Failed to find the belower binary")?;
    let path = scope.unit_path()?;
    let unit = unit(scope, &exe);
    println!("Writing {}:\n\n{}", path.display(), unit);
    if !dry_run {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("Failed to create {}", dir.display()))?;
        }
        std::fs::write(&path, unit).with_context(|| match scope {
            Scope::System => format!(
                "Failed to write {}; run as root, or use --user",
                path.display()
            ),
            Scope::User => format!("Failed to write {}", path.display()),
        })?;
    }
    systemctl(scope, &["daemon-reload"], dry_run)?;
    systemctl(scope, &["enable", "--now", UNIT_NAME], dry_run)?;
    if let Scope::User = scope {
        println!(
            "\nThe recorder runs while you are logged in. To keep it running after you log \
            out, run `loginctl enable-linger`."
        );
    }
    Ok(())
}

pub fn uninstall(scope: Scope, dry_run: bool) -> Result<()> {
    let path = scope.unit_path()?;
    systemctl(scope, &["disable", "--now", UNIT_NAME], dry_run)?;
    println!("Removing {}", path.display());
    if !dry_run {
        std::fs::remove_file(&path)
            .with_context(|| format!("Failed to remove {}", path.display()))?;
    }
    systemctl(scope, &["daemon-reload"], dry_run)?;
    println!("\nRecordings are kept; `belower store info` shows where.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units() {
        let exe = Path::new("/opt/bin/belower");
        let system = unit(Scope::System, exe);
        assert!(system.starts_with("[Unit]\n"));
        assert!(system.contains("\nExecStart=/opt/bin/belower record "));
        assert!(system.contains("\nLogsDirectory=belower\n"));
        assert!(system.contains("\nWantedBy=multi-user.target\n"));

        let user = unit(Scope::User, exe);
        assert!(user.contains("\nExecStart=/opt/bin/belower record "));
        assert!(!user.contains("LogsDirectory="));
        assert!(user.contains("\nWantedBy=default.target\n"));
    }
}
