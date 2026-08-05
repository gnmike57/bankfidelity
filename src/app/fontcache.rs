//! Stage 12 / Item #1: font cache bootstrap.
//!
//! The Stage 11 donor cascade looks up local TTF files via
//! `cache/fonts/manifest.json` to feed Tier 2 (subset extension) and
//! Tier 3 (Gemini Vision typeface ID -> donor selection). Without a
//! populated cache both tiers degrade silently.
//!
//! This module downloads a curated seed of Google Fonts and writes the
//! manifest. The seed was chosen for breadth across the typefaces most
//! commonly seen on retail bank statements:
//!
//!   - Roboto, Open Sans, Noto Sans, Source Sans Pro, Inter - modern sans
//!   - Lato, Montserrat - common transitional sans
//!   - Roboto Slab, Merriweather - slab/serif used by older banks
//!
//! Each font is downloaded directly from the Google Fonts GitHub mirror
//! at a pinned commit hash so this command is deterministic. When a file
//! is already present and the user did NOT pass `--force`, we skip it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One curated entry: canonical typeface name + the URL we fetch from +
/// the local filename to write under `cache/fonts/`.
#[derive(Debug, Clone)]
struct FontSeed {
    /// What we surface in the manifest, used for Gemini Vision matching.
    canonical_name: &'static str,
    /// Direct download URL - pin to a release tag on Google Fonts' repo
    /// so the file content is stable.
    url: &'static str,
    /// Local filename inside the cache dir.
    filename: &'static str,
}

const SEED: &[FontSeed] = &[
    FontSeed {
        canonical_name: "Roboto",
        url: "https://github.com/google/fonts/raw/c19fbf38b574caa75e7ec5b9b73e15053dc54e34/apache/roboto/static/Roboto-Regular.ttf",
        filename: "Roboto-Regular.ttf",
    },
    FontSeed {
        canonical_name: "Open Sans",
        url: "https://github.com/google/fonts/raw/c19fbf38b574caa75e7ec5b9b73e15053dc54e34/ofl/opensans/OpenSans%5Bwdth%2Cwght%5D.ttf",
        filename: "OpenSans-Regular.ttf",
    },
    FontSeed {
        canonical_name: "Noto Sans",
        url: "https://github.com/google/fonts/raw/c19fbf38b574caa75e7ec5b9b73e15053dc54e34/ofl/notosans/NotoSans%5Bwdth%2Cwght%5D.ttf",
        filename: "NotoSans-Regular.ttf",
    },
    FontSeed {
        canonical_name: "Source Sans Pro",
        url: "https://github.com/google/fonts/raw/c19fbf38b574caa75e7ec5b9b73e15053dc54e34/ofl/sourcesans3/SourceSans3%5Bwght%5D.ttf",
        filename: "SourceSansPro-Regular.ttf",
    },
    FontSeed {
        canonical_name: "Inter",
        url: "https://github.com/google/fonts/raw/c19fbf38b574caa75e7ec5b9b73e15053dc54e34/ofl/inter/Inter%5Bopsz%2Cwght%5D.ttf",
        filename: "Inter-Regular.ttf",
    },
    FontSeed {
        canonical_name: "Lato",
        url: "https://github.com/google/fonts/raw/c19fbf38b574caa75e7ec5b9b73e15053dc54e34/ofl/lato/Lato-Regular.ttf",
        filename: "Lato-Regular.ttf",
    },
    FontSeed {
        canonical_name: "Montserrat",
        url: "https://github.com/google/fonts/raw/c19fbf38b574caa75e7ec5b9b73e15053dc54e34/ofl/montserrat/Montserrat%5Bwght%5D.ttf",
        filename: "Montserrat-Regular.ttf",
    },
    FontSeed {
        canonical_name: "Roboto Slab",
        url: "https://github.com/google/fonts/raw/c19fbf38b574caa75e7ec5b9b73e15053dc54e34/apache/robotoslab/RobotoSlab%5Bwght%5D.ttf",
        filename: "RobotoSlab-Regular.ttf",
    },
    FontSeed {
        canonical_name: "Merriweather",
        url: "https://github.com/google/fonts/raw/c19fbf38b574caa75e7ec5b9b73e15053dc54e34/ofl/merriweather/Merriweather%5Bopsz%2Cwdth%2Cwght%5D.ttf",
        filename: "Merriweather-Regular.ttf",
    },
];

/// Output of a bootstrap run, suitable for printing back to the user.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BootstrapReport {
    pub downloaded: Vec<String>,
    pub already_cached: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub manifest_path: PathBuf,
}

impl BootstrapReport {
    pub fn print(&self) {
        for n in &self.downloaded {
            println!("  ✅ downloaded {n}");
        }
        for n in &self.already_cached {
            println!("  • already cached: {n}");
        }
        for (n, e) in &self.failed {
            println!("  ❌ {n}: {e}");
        }
        println!();
        println!(
            "Cache: {} ({} downloaded, {} skipped, {} failed)",
            self.manifest_path
                .parent()
                .unwrap_or(Path::new("?"))
                .display(),
            self.downloaded.len(),
            self.already_cached.len(),
            self.failed.len()
        );
    }
}

/// Run the bootstrap. Creates the cache directory if necessary, downloads
/// each entry of [`SEED`] via the blocking `reqwest::blocking::Client` (we
/// can't pull tokio in here - this CLI subcommand runs synchronously), and
/// writes `manifest.json`.
pub fn bootstrap(cache_dir: &Path, force: bool) -> Result<BootstrapReport, String> {
    std::fs::create_dir_all(cache_dir).map_err(|e| format!("create cache dir: {e}"))?;

    let client = reqwest::blocking::Client::builder()
        .user_agent("dual-core-pdf-pipeline/0.4 fontcache-init")
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let mut report = BootstrapReport {
        manifest_path: cache_dir.join("manifest.json"),
        ..Default::default()
    };

    let mut manifest: BTreeMap<String, String> = if report.manifest_path.exists() && !force {
        std::fs::read_to_string(&report.manifest_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        BTreeMap::new()
    };

    for seed in SEED {
        let dest = cache_dir.join(seed.filename);
        if dest.exists() && !force {
            // Stage 13 / Item #13: validate the cached file before trusting
            // it. If it's < 4KB or doesn't have a TTF magic prefix, treat
            // it as a partial download and re-fetch.
            let valid = match std::fs::read(&dest) {
                Ok(bytes) if bytes.len() >= 4096 => {
                    let magic = &bytes[..4];
                    matches!(magic, b"\x00\x01\x00\x00" | b"true" | b"OTTO" | b"ttcf")
                }
                _ => false,
            };
            if valid {
                manifest.insert(seed.canonical_name.into(), seed.filename.into());
                report.already_cached.push(seed.canonical_name.into());
                continue;
            }
            // Otherwise fall through to a fresh download.
        }

        match download_one(&client, seed.url, &dest) {
            Ok(()) => {
                manifest.insert(seed.canonical_name.into(), seed.filename.into());
                report.downloaded.push(seed.canonical_name.into());
            }
            Err(e) => {
                report.failed.push((seed.canonical_name.into(), e));
            }
        }
    }

    let manifest_json =
        serde_json::to_string_pretty(&manifest).map_err(|e| format!("manifest serialize: {e}"))?;
    std::fs::write(&report.manifest_path, manifest_json)
        .map_err(|e| format!("manifest write: {e}"))?;

    Ok(report)
}

fn download_one(client: &reqwest::blocking::Client, url: &str, dest: &Path) -> Result<(), String> {
    // Stage 13 / Item #13: download to a temp side-file first so a partial
    // failure never leaves a half-written TTF in the cache that subsequent
    // runs would treat as "already cached and good".
    let tmp = dest.with_extension("ttf.partial");
    if tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
    }

    let resp = client
        .get(url)
        .send()
        .map_err(|e| format!("http request: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| format!("read body: {e}"))?;
    // Reject anything smaller than 4KB - any plausible TTF is many KB.
    if bytes.len() < 4096 {
        return Err(format!("response too small ({} bytes)", bytes.len()));
    }
    // Verify TTF/OTF magic so we don't write HTML/error pages to disk.
    let magic = &bytes[..4.min(bytes.len())];
    let is_truetype = matches!(magic, b"\x00\x01\x00\x00" | b"true" | b"OTTO" | b"ttcf");
    if !is_truetype {
        return Err(format!(
            "downloaded data does not look like a TTF/OTF (first 4 bytes: {magic:02x?})"
        ));
    }
    std::fs::write(&tmp, &bytes).map_err(|e| format!("write tmp: {e}"))?;
    // Atomic rename so a concurrent reader either sees the old file or
    // the new one, never a torn write.
    if dest.exists() {
        let _ = std::fs::remove_file(dest);
    }
    std::fs::rename(&tmp, dest).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Default cache directory, matching the convention used by
/// `python/font_replicator.py`.
pub fn default_cache_dir() -> PathBuf {
    PathBuf::from("cache").join("fonts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_path_is_inside_cache_dir() {
        let dir = std::env::temp_dir().join(format!("dcpp-fontcache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let report = BootstrapReport {
            manifest_path: dir.join("manifest.json"),
            ..Default::default()
        };
        assert!(report.manifest_path.starts_with(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[allow(clippy::const_is_empty)]
    fn seed_list_is_non_empty() {
        assert!(!SEED.is_empty());
        for s in SEED {
            assert!(
                s.url.starts_with("https://"),
                "{} url not https",
                s.canonical_name
            );
            assert!(
                s.filename.ends_with(".ttf"),
                "{} filename not .ttf",
                s.canonical_name
            );
        }
    }
}
