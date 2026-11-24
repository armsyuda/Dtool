use std::{io::Read, time::Duration};

pub const RELEASES_PAGE: &str = "https://github.com/armsyuda/Dtool/releases/latest";
const RELEASE_API: &str = "https://api.github.com/repos/armsyuda/Dtool/releases/latest";

fn version(text: &str) -> Option<[u32; 3]> {
    let mut parts = text.strip_prefix('v').unwrap_or(text).split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().map(str::parse).transpose().ok()?.unwrap_or(0);
    parts.next().is_none().then_some([major, minor, patch])
}

/// Check for a published release without downloading assets or collecting machine information.
pub fn newer_release(current: &str) -> Result<Option<String>, ()> {
    let current = version(current).ok_or(())?;
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(6)).redirects(0).build();
    let response = match agent.get(RELEASE_API)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", concat!("DTool/", env!("CARGO_PKG_VERSION")))
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(_) => return Err(()),
    };
    let mut bytes = Vec::new();
    response.into_reader().take(128 * 1024).read_to_end(&mut bytes).map_err(|_| ())?;
    let data: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let tag = data["tag_name"].as_str().ok_or(())?;
    let latest = version(tag).ok_or(())?;
    Ok((latest > current).then(|| tag.trim_start_matches('v').to_owned()))
}
