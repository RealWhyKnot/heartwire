use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{Package, Release, agent, asset_url};

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

pub fn download(
    release: &Release,
    package: Package,
    staging: &Path,
    progress: &dyn Fn(f32),
) -> Result<PathBuf, String> {
    let archive = package.asset(&release.tag_name);
    let integrity = package.integrity(&release.tag_name);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::{check, testing::scratch};
    use crate::version::Channel;

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
    #[ignore = "downloads the latest release from GitHub"]
    fn downloads_and_verifies_a_real_release() {
        let release = check("v2000.1.1.0", Channel::Release)
            .expect("the releases API answers")
            .expect("a release with assets for this platform");
        let staging = scratch("download");
        let archive = download(&release, Package::Archive, &staging, &|_| {})
            .expect("download passes the integrity check");
        assert!(archive.metadata().unwrap().len() > 100_000);
        let _ = std::fs::remove_dir_all(&staging);
    }
}
