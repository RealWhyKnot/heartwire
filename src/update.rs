use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::version::Channel;

const RELEASES_URL: &str =
    "https://api.github.com/repos/RealWhyKnot/hr-osc-rust/releases?per_page=20";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AppVersion {
    year: u32,
    month: u32,
    day: u32,
    revision: u32,
    stable: bool,
}

impl AppVersion {
    pub fn parse(text: &str) -> Option<AppVersion> {
        let text = text.trim();
        let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
        let (numbers, suffix) = text.split_once('-').unwrap_or((text, ""));
        let mut parts = numbers.split('.').map(|p| p.parse::<u32>().ok());
        let version = AppVersion {
            year: parts.next()??,
            month: parts.next()??,
            day: parts.next()??,
            revision: parts.next()??,
            stable: suffix.is_empty(),
        };
        parts.next().is_none().then_some(version)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

pub fn select(releases: &[Release], current: AppVersion, channel: Channel) -> Option<&Release> {
    if channel == Channel::Dev {
        return None;
    }
    let mut best: Option<(&Release, AppVersion)> = None;
    for release in releases {
        if release.draft || (channel == Channel::Release && release.prerelease) {
            continue;
        }
        let Some(version) = AppVersion::parse(&release.tag_name) else {
            continue;
        };
        if version > best.map_or(current, |b| b.1) {
            best = Some((release, version));
        }
    }
    best.map(|b| b.0)
}

pub fn rid() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "win-arm64",
        ("windows", _) => "win-x64",
        ("macos", "aarch64") => "osx-arm64",
        ("macos", _) => "osx-x64",
        (_, "aarch64") => "linux-arm64",
        _ => "linux-x64",
    }
}

fn base_name(tag: &str) -> String {
    let version = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
    format!("hr-osc-rust-{version}-{}", rid())
}

pub fn archive_name(tag: &str) -> String {
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    format!("{}.{ext}", base_name(tag))
}

pub fn integrity_name(tag: &str) -> String {
    format!("{}.integrity.tsv", base_name(tag))
}

pub fn parse_integrity(tsv: &str, archive: &str) -> Result<(String, u64), String> {
    let line = tsv
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .find(|l| !l.is_empty())
        .ok_or("integrity file is empty")?;
    let fields: Vec<&str> = line.split('\t').collect();
    let [hash, size, name] = fields[..] else {
        return Err(format!(
            "integrity row has {} fields, expected 3",
            fields.len()
        ));
    };
    if name != archive {
        return Err(format!("integrity row names {name}, expected {archive}"));
    }
    let hash = hash.trim().to_ascii_lowercase();
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("integrity hash is not 64 hex characters".into());
    }
    let size: u64 = size
        .trim()
        .parse()
        .ok()
        .filter(|s| *s > 0)
        .ok_or_else(|| format!("integrity size {size} is not a positive number"))?;
    Ok((hash, size))
}

fn agent() -> ureq::Agent {
    crate::net::agent(Duration::from_secs(60))
}

pub fn check(current: &str, channel: Channel) -> Result<Option<Release>, String> {
    let Some(current) = AppVersion::parse(current) else {
        return Ok(None);
    };
    if channel == Channel::Dev {
        return Ok(None);
    }
    let text = agent()
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .with_config()
        .limit(4 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let releases: Vec<Release> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let Some(release) = select(&releases, current, channel) else {
        return Ok(None);
    };
    let archive = archive_name(&release.tag_name);
    let integrity = integrity_name(&release.tag_name);
    let has = |n: &str| release.assets.iter().any(|a| a.name == n);
    Ok((has(&archive) && has(&integrity)).then(|| release.clone()))
}

fn asset_url<'a>(release: &'a Release, name: &str) -> Result<&'a str, String> {
    release
        .assets
        .iter()
        .find(|a| a.name == name)
        .map(|a| a.browser_download_url.as_str())
        .ok_or_else(|| format!("release {} has no {name}", release.tag_name))
}

pub fn download(
    release: &Release,
    staging: &Path,
    progress: &dyn Fn(f32),
) -> Result<PathBuf, String> {
    let archive = archive_name(&release.tag_name);
    let integrity = integrity_name(&release.tag_name);
    let agent = agent();
    let tsv = agent
        .get(asset_url(release, &integrity)?)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let (hash, size) = parse_integrity(&tsv, &archive)?;
    if staging.exists() {
        std::fs::remove_dir_all(staging).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(staging).map_err(|e| e.to_string())?;
    let path = staging.join(&archive);
    let mut response = agent
        .get(asset_url(release, &archive)?)
        .call()
        .map_err(|e| e.to_string())?;
    let mut reader = response.body_mut().with_config().limit(size + 1).reader();
    let mut file = File::create(&path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buf[..n]);
        total += n as u64;
        progress((total as f32 / size as f32).min(1.0));
    }
    file.flush().map_err(|e| e.to_string())?;
    if total != size {
        return Err(format!("{archive} is {total} bytes, expected {size}"));
    }
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != hash {
        return Err(format!("{archive} sha256 {actual} does not match {hash}"));
    }
    Ok(path)
}

fn ps_quote(value: &Path) -> String {
    format!("'{}'", value.display().to_string().replace('\'', "''"))
}

fn sh_quote(value: &Path) -> String {
    format!("'{}'", value.display().to_string().replace('\'', "'\\''"))
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
    [
        "$ErrorActionPreference = 'Stop'".to_owned(),
        "$applied = $false".into(),
        "try {".into(),
        format!("    Wait-Process -Id {pid} -Timeout 120 -ErrorAction SilentlyContinue"),
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
    ]
    .join("\r\n")
        + "\r\n"
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

pub fn apply(archive: &Path, staging: &Path, log: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let install = exe.parent().ok_or("no install folder")?.to_path_buf();
    let pid = std::process::id();
    let mut command = if cfg!(windows) {
        let script = staging.join("apply.ps1");
        std::fs::write(
            &script,
            powershell_script(pid, archive, staging, &install, &exe, log),
        )
        .map_err(|e| e.to_string())?;
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
        std::fs::write(
            &script,
            sh_script(pid, archive, staging, &install, &exe, log),
        )
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
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool) -> Release {
        Release {
            tag_name: tag.into(),
            draft: false,
            prerelease,
            assets: Vec::new(),
        }
    }

    #[test]
    fn versions_order_by_date_then_revision_then_stability() {
        let v = |t| AppVersion::parse(t).unwrap();
        assert!(v("v2026.10.9.1") > v("v2026.10.9.0"));
        assert!(v("2026.10.10.0") > v("v2026.10.9.5"));
        assert!(v("v2026.10.9.0") > v("v2026.10.9.0-beta"));
        assert!(v("v2026.11.1.0-beta") > v("v2026.10.31.3"));
        assert_eq!(AppVersion::parse("dev"), None);
        assert_eq!(AppVersion::parse("v2026.10.9"), None);
        assert_eq!(AppVersion::parse("v2026.10.9.0.1"), None);
    }

    #[test]
    fn release_channel_skips_betas() {
        let list = vec![
            release("v2026.10.12.0-beta", true),
            release("v2026.10.11.0", false),
            release("v2026.10.8.0", false),
        ];
        let current = AppVersion::parse("v2026.10.9.0").unwrap();
        assert_eq!(
            select(&list, current, Channel::Release).unwrap().tag_name,
            "v2026.10.11.0"
        );
        assert_eq!(
            select(&list, current, Channel::Beta).unwrap().tag_name,
            "v2026.10.12.0-beta"
        );
        assert!(select(&list, current, Channel::Dev).is_none());
        let newest = AppVersion::parse("v2026.10.12.0").unwrap();
        assert!(select(&list, newest, Channel::Beta).is_none());
    }

    #[test]
    fn drafts_are_ignored() {
        let mut draft = release("v2027.1.1.0", false);
        draft.draft = true;
        let current = AppVersion::parse("v2026.1.1.0").unwrap();
        assert!(select(&[draft], current, Channel::Release).is_none());
    }

    #[test]
    fn asset_names_follow_the_rid() {
        let name = archive_name("v2026.10.9.0");
        assert!(name.starts_with("hr-osc-rust-2026.10.9.0-"));
        assert!(name.ends_with(if cfg!(windows) { ".zip" } else { ".tar.gz" }));
        assert_eq!(
            integrity_name("v2026.10.9.0-beta"),
            format!("hr-osc-rust-2026.10.9.0-beta-{}.integrity.tsv", rid())
        );
    }

    #[test]
    fn integrity_rows() {
        let hash = "a".repeat(64);
        let tsv = format!("{hash}\t1234\tapp.zip\n{}\t5\tapp.exe\n", "b".repeat(64));
        assert_eq!(parse_integrity(&tsv, "app.zip"), Ok((hash.clone(), 1234)));
        assert!(parse_integrity(&tsv, "other.zip").is_err());
        assert!(parse_integrity("", "app.zip").is_err());
        assert!(parse_integrity(&format!("{hash}\t0\tapp.zip"), "app.zip").is_err());
        assert!(parse_integrity("xyz\t5\tapp.zip", "app.zip").is_err());
        assert!(parse_integrity(&format!("{hash}\t5"), "app.zip").is_err());
    }

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
    }
}
