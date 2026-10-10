use std::path::Path;
use std::process::Command;

use super::Package;

fn ps_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn ps_quote(value: &Path) -> String {
    ps_text(&value.display().to_string())
}

fn sh_quote(value: &Path) -> String {
    format!("'{}'", value.display().to_string().replace('\'', "'\\''"))
}

fn powershell_helper(
    pid: u32,
    body: Vec<String>,
    staging: &Path,
    install: &Path,
    exe: &Path,
    log: &Path,
) -> String {
    let mut lines = vec![
        "$ErrorActionPreference = 'Stop'".to_owned(),
        "$applied = $false".into(),
        "try {".into(),
        format!("    Wait-Process -Id {pid} -Timeout 120 -ErrorAction SilentlyContinue"),
    ];
    lines.extend(body);
    lines.extend([
        "    $applied = $true".into(),
        "} catch {".into(),
        format!(
            "    'update apply failed' | Add-Content -LiteralPath {}",
            ps_quote(log)
        ),
        format!(
            "    $_ | Out-String | Add-Content -LiteralPath {}",
            ps_quote(log)
        ),
        "}".into(),
        "try {".into(),
        format!(
            "    Start-Process -FilePath {} -WorkingDirectory {}",
            ps_quote(exe),
            ps_quote(install)
        ),
        "} catch {".into(),
        format!(
            "    $_ | Out-String | Add-Content -LiteralPath {}",
            ps_quote(log)
        ),
        "}".into(),
        format!(
            "Remove-Item -LiteralPath {} -Recurse -Force -ErrorAction SilentlyContinue",
            ps_quote(staging)
        ),
        "if (-not $applied) { exit 1 }".into(),
    ]);
    lines.join("\r\n") + "\r\n"
}

pub fn powershell_script(
    pid: u32,
    archive: &Path,
    staging: &Path,
    install: &Path,
    exe: &Path,
    log: &Path,
) -> String {
    let extracted = staging.join("extracted");
    let body = vec![
        format!(
            "    Expand-Archive -LiteralPath {} -DestinationPath {} -Force",
            ps_quote(archive),
            ps_quote(&extracted)
        ),
        "    $attempt = 0".into(),
        "    while ($true) {".into(),
        "        try {".into(),
        format!(
            "            Copy-Item -Path (Join-Path {} '*') -Destination {} -Recurse -Force",
            ps_quote(&extracted),
            ps_quote(install)
        ),
        "            break".into(),
        "        } catch {".into(),
        "            $attempt++".into(),
        "            if ($attempt -ge 10) { throw }".into(),
        "            Start-Sleep -Seconds 1".into(),
        "        }".into(),
        "    }".into(),
    ];
    powershell_helper(pid, body, staging, install, exe, log)
}

pub fn setup_script(
    pid: u32,
    setup: &Path,
    staging: &Path,
    install: &Path,
    exe: &Path,
    log: &Path,
) -> String {
    let body = vec![
        format!(
            "    $setup = Start-Process -FilePath {} -ArgumentList {} -Wait -PassThru",
            ps_quote(setup),
            ps_text(&format!("/S /D={}", install.display()))
        ),
        "    if ($setup.ExitCode -ne 0) { throw \"setup exited with code $($setup.ExitCode)\" }"
            .into(),
    ];
    powershell_helper(pid, body, staging, install, exe, log)
}

pub fn sh_script(
    pid: u32,
    archive: &Path,
    staging: &Path,
    install: &Path,
    exe: &Path,
    log: &Path,
) -> String {
    let extracted = staging.join("extracted");
    [
        "#!/bin/sh".to_owned(),
        format!("pid={pid}"),
        format!("archive={}", sh_quote(archive)),
        format!("extracted={}", sh_quote(&extracted)),
        format!("staging={}", sh_quote(staging)),
        format!("install={}", sh_quote(install)),
        format!("exe={}", sh_quote(exe)),
        format!("log={}", sh_quote(log)),
        "i=0".into(),
        "while kill -0 \"$pid\" 2>/dev/null; do".into(),
        "  i=$((i+1))".into(),
        "  if [ \"$i\" -ge 120 ]; then break; fi".into(),
        "  sleep 1".into(),
        "done".into(),
        "mkdir -p \"$extracted\"".into(),
        "if tar -xzf \"$archive\" -C \"$extracted\" 2>>\"$log\" && cp -R \"$extracted/.\" \"$install/\" 2>>\"$log\"; then".into(),
        "  chmod +x \"$exe\" 2>>\"$log\"".into(),
        "else".into(),
        "  echo 'update apply failed' >>\"$log\"".into(),
        "fi".into(),
        "cd \"$install\" && nohup \"$exe\" >/dev/null 2>>\"$log\" &".into(),
        "rm -rf \"$staging\"".into(),
    ]
    .join("\n")
        + "\n"
}

pub fn apply(package: Package, file: &Path, staging: &Path, log: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let install = exe.parent().ok_or("no install folder")?.to_path_buf();
    launch_helper(
        std::process::id(),
        package,
        file,
        staging,
        &install,
        &exe,
        log,
    )
    .map(|_| ())
}

fn launch_helper(
    pid: u32,
    package: Package,
    file: &Path,
    staging: &Path,
    install: &Path,
    exe: &Path,
    log: &Path,
) -> Result<std::process::Child, String> {
    let mut command = if cfg!(windows) {
        let script = staging.join("apply.ps1");
        let text = match package {
            Package::Archive => powershell_script(pid, file, staging, install, exe, log),
            Package::Setup => setup_script(pid, file, staging, install, exe, log),
        };
        std::fs::write(&script, text).map_err(|e| e.to_string())?;
        let mut c = Command::new("powershell.exe");
        c.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
        ])
        .arg(&script);
        c
    } else {
        let script = staging.join("apply.sh");
        std::fs::write(&script, sh_script(pid, file, staging, install, exe, log))
            .map_err(|e| e.to_string())?;
        let mut c = Command::new("/bin/sh");
        c.arg(&script);
        c
    };
    command.current_dir(std::env::temp_dir());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.spawn().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::testing::scratch;
    use std::path::PathBuf;

    #[test]
    fn scripts_quote_paths() {
        let p = Path::new("C:/it's here/app.zip");
        let ps = powershell_script(
            42,
            p,
            Path::new("C:/s"),
            Path::new("C:/i"),
            Path::new("C:/i/a.exe"),
            Path::new("C:/l.log"),
        );
        assert!(ps.contains("Wait-Process -Id 42"));
        assert!(ps.contains("'C:/it''s here/app.zip'"));
        let sh = sh_script(
            42,
            p,
            Path::new("/s"),
            Path::new("/i"),
            Path::new("/i/a"),
            Path::new("/l"),
        );
        assert!(sh.contains("archive='C:/it'\\''s here/app.zip'"));
        assert!(sh.starts_with("#!/bin/sh\n"));
        let setup = setup_script(
            42,
            Path::new("C:/s/heartwire-setup.exe"),
            Path::new("C:/s"),
            Path::new("C:/it's here"),
            Path::new("C:/it's here/heartwire.exe"),
            Path::new("C:/l.log"),
        );
        assert!(setup.contains("-ArgumentList '/S /D=C:/it''s here' -Wait -PassThru"));
        assert!(setup.contains("Start-Process -FilePath 'C:/it''s here/heartwire.exe'"));
    }

    #[cfg(windows)]
    fn run_setup(root: &Path, exit_code: u8) -> (bool, PathBuf, PathBuf) {
        let staging = root.join("staging");
        let install = root.join("install dir");
        for dir in [&staging, &install] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let setup = staging.join("setup.cmd");
        let args = root.join("args.txt");
        std::fs::write(
            &setup,
            format!(
                "@echo %*> \"{}\"\r\n@exit /b {exit_code}\r\n",
                args.display()
            ),
        )
        .unwrap();
        let log = root.join("update.log");
        let exe = PathBuf::from(r"C:\Windows\System32\rundll32.exe");
        let mut child = launch_helper(
            2_147_483_000,
            Package::Setup,
            &setup,
            &staging,
            &install,
            &exe,
            &log,
        )
        .unwrap();
        let ok = child.wait().unwrap().success();
        assert!(!staging.exists(), "the staging folder is removed");
        (ok, install, log)
    }

    #[cfg(windows)]
    #[test]
    fn helper_runs_the_setup_silently_into_the_install_folder() {
        let root = scratch("setup");
        let (ok, install, log) = run_setup(&root, 0);
        assert!(ok, "{:?}", std::fs::read_to_string(&log));
        assert_eq!(
            std::fs::read_to_string(root.join("args.txt"))
                .unwrap()
                .trim(),
            format!("/S /D={}", install.display())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn helper_logs_a_failed_setup() {
        let root = scratch("setup-failed");
        let (ok, _, log) = run_setup(&root, 5);
        assert!(!ok);
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("setup exited with code 5"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn helper_installs_the_archive_and_cleans_up() {
        let root = scratch("helper");
        let payload = root.join("payload");
        let staging = root.join("staging");
        let install = root.join("install");
        for dir in [&payload, &staging, &install] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(payload.join("version.txt"), "new").unwrap();
        std::fs::write(install.join("version.txt"), "old").unwrap();
        let (archive, exe) = if cfg!(windows) {
            let archive = staging.join("update.zip");
            let status = Command::new("powershell.exe")
                .args(["-NoProfile", "-Command"])
                .arg(format!(
                    "Compress-Archive -Path '{}' -DestinationPath '{}'",
                    payload.join("*").display(),
                    archive.display()
                ))
                .status()
                .unwrap();
            assert!(status.success());
            (archive, PathBuf::from(r"C:\Windows\System32\rundll32.exe"))
        } else {
            let archive = staging.join("update.tar.gz");
            let status = Command::new("tar")
                .arg("-czf")
                .arg(&archive)
                .arg("-C")
                .arg(&payload)
                .arg(".")
                .status()
                .unwrap();
            assert!(status.success());
            (archive, PathBuf::from("/usr/bin/true"))
        };
        let log = root.join("update.log");
        let mut child = launch_helper(
            2_147_483_000,
            Package::Archive,
            &archive,
            &staging,
            &install,
            &exe,
            &log,
        )
        .unwrap();
        assert!(
            child.wait().unwrap().success(),
            "{:?}",
            std::fs::read_to_string(&log)
        );
        assert_eq!(
            std::fs::read_to_string(install.join("version.txt")).unwrap(),
            "new"
        );
        assert!(!staging.exists(), "the staging folder is removed");
        let _ = std::fs::remove_dir_all(&root);
    }
}
