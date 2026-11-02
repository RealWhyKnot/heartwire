use std::path::Path;
use std::process::Command;

use super::Package;

const POWERSHELL: &str = include_str!("Invoke-Update.ps1");
const BASH: &str = include_str!("apply.sh");

struct Update<'a> {
    pid: u32,
    package: Package,
    file: &'a Path,
    staging: &'a Path,
    install: &'a Path,
    exe: &'a Path,
    log: &'a Path,
}

impl Update<'_> {
    fn command(&self, windows: bool, script: &Path) -> Command {
        let mut command = if windows {
            let mut command = Command::new("powershell.exe");
            command.args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-WindowStyle",
                "Hidden",
                "-File",
            ]);
            command
                .arg(script)
                .arg("-ParentProcessId")
                .arg(self.pid.to_string());
            command.arg("-Package").arg(match self.package {
                Package::Archive => "Archive",
                Package::Setup => "Setup",
            });
            for (name, path) in [
                ("-File", self.file),
                ("-Staging", self.staging),
                ("-Install", self.install),
                ("-Executable", self.exe),
                ("-Log", self.log),
            ] {
                command.arg(name).arg(path);
            }
            command
        } else {
            let mut command = Command::new("bash");
            command.arg(script).arg(self.pid.to_string());
            command.args([self.file, self.staging, self.install, self.exe, self.log]);
            command
        };
        command.current_dir(std::env::temp_dir());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        command
    }
}

pub(crate) fn apply(
    package: Package,
    file: &Path,
    staging: &Path,
    log: &Path,
) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| {
        format!("locate Heartwire to install the update: {error}. Try the update again.")
    })?;
    let install = exe.parent().ok_or("no install folder")?;
    let update = Update {
        pid: std::process::id(),
        package,
        file,
        staging,
        install,
        exe: &exe,
        log,
    };
    let (name, text) = if cfg!(windows) {
        ("apply.ps1", POWERSHELL)
    } else {
        ("apply.sh", BASH)
    };
    let script = staging.join(name);
    std::fs::write(&script, text).map_err(|error| {
        format!(
            "write the update helper {}: {error}. Check that the folder is writable.",
            script.display()
        )
    })?;
    update
        .command(cfg!(windows), &script)
        .spawn()
        .map_err(|error| {
            format!(
                "start the update helper {}: {error}. Try the update again.",
                script.display()
            )
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn paths_remain_separate_arguments_for_both_shells() {
        for package in [Package::Archive, Package::Setup] {
            let update = Update {
                pid: 42,
                package,
                file: Path::new("C:/it's here/update.zip"),
                staging: Path::new("C:/it's here/staging"),
                install: Path::new("C:/it's here/install dir"),
                exe: Path::new("C:/it's here/install dir/heartwire.exe"),
                log: Path::new("C:/it's here/update.log"),
            };
            let script = update.staging.join("apply.ps1");
            let command = update.command(true, &script);
            let args: Vec<_> = command.get_args().collect();
            for (name, path) in [
                ("-File", update.file),
                ("-Staging", update.staging),
                ("-Install", update.install),
                ("-Executable", update.exe),
                ("-Log", update.log),
            ] {
                assert!(
                    args.windows(2)
                        .any(|pair| pair == [OsStr::new(name), path.as_os_str()])
                );
            }
            assert_eq!(args[6], script.as_os_str());
            assert_eq!(args[8], "42");
            assert_eq!(
                args[10],
                if package == Package::Setup {
                    "Setup"
                } else {
                    "Archive"
                }
            );
            let script = update.staging.join("apply.sh");
            let command = update.command(false, &script);
            let args: Vec<_> = command.get_args().collect();
            assert_eq!(
                args,
                [
                    script.as_os_str(),
                    OsStr::new("42"),
                    update.file.as_os_str(),
                    update.staging.as_os_str(),
                    update.install.as_os_str(),
                    update.exe.as_os_str(),
                    update.log.as_os_str()
                ]
            );
        }
    }
}
