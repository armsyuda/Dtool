use sha2::{Digest, Sha256};
use std::{io::Read, time::Duration};

const RELEASE_API: &str = "https://api.github.com/repos/armsyuda/Dtool/releases/latest";
const RELEASE_DOWNLOAD: &str = "https://github.com/armsyuda/Dtool/releases/download";
const MAX_INSTALLER_BYTES: u64 = 1024 * 1024 * 1024;

pub struct ReleaseUpdate {
    pub version: String,
    pub notes: String,
    installer: Option<InstallerAsset>,
}

struct InstallerAsset {
    url: String,
    bytes: u64,
    sha256: String,
}

pub struct DownloadedInstaller {
    directory: tempfile::TempDir,
    path: std::path::PathBuf,
}

fn version(text: &str) -> Option<[u32; 3]> {
    let mut parts = text.strip_prefix('v').unwrap_or(text).split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().map(str::parse).transpose().ok()?.unwrap_or(0);
    parts.next().is_none().then_some([major, minor, patch])
}

/// Check for a published release without downloading assets or collecting machine information.
pub fn newer_release(current: &str) -> Result<Option<ReleaseUpdate>, ()> {
    let current = version(current).ok_or(())?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(6))
        .redirects(0)
        .build();
    let response = match agent
        .get(RELEASE_API)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", concat!("DTool/", env!("CARGO_PKG_VERSION")))
        .call()
    {
        Ok(response) => response,
        Err(_) => return Err(()),
    };
    let data: serde_json::Value =
        serde_json::from_reader(response.into_reader().take(512 * 1024)).map_err(|_| ())?;
    let tag = data["tag_name"].as_str().ok_or(())?;
    let latest = version(tag).ok_or(())?;
    if latest <= current || data["draft"] == true || data["prerelease"] == true {
        return Ok(None);
    }
    let version = tag.trim_start_matches('v').to_owned();
    let name = format!("Dtool-{version}-Windows-x64-Setup.exe");
    let installer = data["assets"].as_array().and_then(|assets| {
        let asset = assets
            .iter()
            .find(|asset| asset["name"] == name && asset["state"] == "uploaded")?;
        let bytes = asset["size"]
            .as_u64()
            .filter(|bytes| (1..=MAX_INSTALLER_BYTES).contains(bytes))?;
        let sha256 = asset["digest"].as_str()?.strip_prefix("sha256:")?;
        if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        Some(InstallerAsset {
            url: format!("{RELEASE_DOWNLOAD}/{tag}/{name}"),
            bytes,
            sha256: sha256.to_ascii_lowercase(),
        })
    });
    let notes = data["body"].as_str().unwrap_or_default().to_owned();
    Ok(Some(ReleaseUpdate {
        version,
        notes,
        installer,
    }))
}

impl ReleaseUpdate {
    pub fn can_install(&self) -> bool {
        cfg!(all(windows, target_arch = "x86_64")) && self.installer.is_some()
    }

    pub fn download(&self, progress: impl Fn(u64, u64)) -> Result<DownloadedInstaller, ()> {
        use std::io::Write;

        let asset = self
            .installer
            .as_ref()
            .filter(|_| self.can_install())
            .ok_or(())?;
        let response = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(600))
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(30))
            .build()
            .get(&asset.url)
            .set("User-Agent", concat!("DTool/", env!("CARGO_PKG_VERSION")))
            .call()
            .map_err(|_| ())?;
        let directory = tempfile::Builder::new()
            .prefix("dtool-update-")
            .tempdir()
            .map_err(|_| ())?;
        let path = directory.path().join("DTool-Setup.exe");
        let mut file = std::fs::File::create_new(&path).map_err(|_| ())?;
        let mut input = response.into_reader().take(asset.bytes + 1);
        let mut sha256 = Sha256::new();
        let mut buffer = vec![0; 64 * 1024];
        let mut received = 0;
        loop {
            let count = input.read(&mut buffer).map_err(|_| ())?;
            if count == 0 {
                break;
            }
            let chunk = &buffer[..count];
            file.write_all(chunk).map_err(|_| ())?;
            sha256.update(chunk);
            received += count as u64;
            progress(received, asset.bytes);
        }
        let digest: String = sha256
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if received != asset.bytes || digest != asset.sha256 {
            return Err(());
        }
        file.flush().map_err(|_| ())?;
        Ok(DownloadedInstaller { directory, path })
    }
}

impl DownloadedInstaller {
    pub fn launch(self) -> std::io::Result<()> {
        let mut command = std::process::Command::new(&self.path);
        #[cfg(windows)]
        if let Ok(exe) = std::env::current_exe()
            && let Some(directory) = exe
                .parent()
                .filter(|path| path.join("Uninstall.exe").is_file())
        {
            use std::os::windows::process::CommandExt;

            // NSIS takes /D as an unquoted final argument, including spaces in the path.
            let mut destination = std::ffi::OsString::from("/D=");
            destination.push(directory);
            command
                .arg("/S")
                .arg(format!("/DTOOLUPDATE={}", std::process::id()))
                .raw_arg(destination);
        }
        command.spawn()?;
        // Windows keeps this file open while setup is running.
        let _ = self.directory.keep();
        Ok(())
    }
}
