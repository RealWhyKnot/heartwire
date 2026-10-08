mod download;
mod install;

use std::time::Duration;

use serde::Deserialize;

use crate::version::Channel;

pub use download::download;
pub use install::apply;

const RELEASES_URL: &str =
    "https://api.github.com/repos/RealWhyKnot/heartwire/releases?per_page=20";

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
    format!("heartwire-{version}-{}", rid())
}

pub fn archive_name(tag: &str) -> String {
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    format!("{}.{ext}", base_name(tag))
}

pub fn integrity_name(tag: &str) -> String {
    format!("{}.integrity.tsv", base_name(tag))
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

#[cfg(test)]
pub(crate) mod testing {
    use std::path::PathBuf;

    pub fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("heartwire-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
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
        assert!(name.starts_with("heartwire-2026.10.9.0-"));
        assert!(name.ends_with(if cfg!(windows) { ".zip" } else { ".tar.gz" }));
        assert_eq!(
            integrity_name("v2026.10.9.0-beta"),
            format!("heartwire-2026.10.9.0-beta-{}.integrity.tsv", rid())
        );
    }
}
