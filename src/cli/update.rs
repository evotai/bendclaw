use std::fs;
use std::io::Cursor;
use std::io::IsTerminal;
use std::path::Path;

use anyhow::Context as _;
use anyhow::Result;
use flate2::read::GzDecoder;
use serde::Deserialize;

const GITHUB_API: &str = "https://api.github.com";
const GITHUB_REPO: &str = "EvotAI/bendclaw";
const BINARY_NAME: &str = "bendclaw";
const DRACULA_GREEN: (u8, u8, u8) = (80, 250, 123);

#[derive(Debug, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubReleaseAsset {
    pub name: String,
    pub url: String,
}

pub async fn cmd_update() -> Result<()> {
    let style = UpdateCliStyle::detect();
    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    let current_tag = current_release_tag();
    let target = supported_target()?;

    println!("Current version: {}", current_tag.trim_start_matches('v'));
    println!("Checking for updates to latest stable version...");
    let release = fetch_latest_release().await?;
    let latest_tag = release.tag_name.clone();
    let latest_display = latest_tag.trim_start_matches('v');
    let current_display = current_tag.trim_start_matches('v');

    if tags_match(&current_tag, &latest_tag) {
        println!(
            "{}",
            style.success(format!("Already up to date: {latest_display}"))
        );
        return Ok(());
    }

    let asset = select_asset(&release, target).with_context(|| {
        format!(
            "release {} does not contain an asset for target {}",
            latest_tag, target
        )
    })?;

    println!(
        "New version available: {} (current: {})",
        latest_display, current_display
    );
    println!("Installing update...");
    println!("Using release asset update method...");
    let archive = download_asset(asset).await?;
    let binary = extract_binary(&archive)
        .with_context(|| format!("failed to extract {BINARY_NAME} from release archive"))?;
    install_binary(&current_exe, &binary)?;

    println!(
        "{}",
        style.success(format!(
            "Successfully updated from {} to version {}",
            current_display, latest_display
        ))
    );
    println!("Installed binary: {}", current_exe.display());
    Ok(())
}

struct UpdateCliStyle {
    ansi_enabled: bool,
}

impl UpdateCliStyle {
    fn detect() -> Self {
        Self {
            ansi_enabled: stdout_supports_color(),
        }
    }

    fn success(&self, message: impl AsRef<str>) -> String {
        self.paint(message.as_ref(), DRACULA_GREEN)
    }

    fn paint(&self, message: &str, (r, g, b): (u8, u8, u8)) -> String {
        if !self.ansi_enabled {
            return message.to_string();
        }

        format!("\x1b[38;2;{r};{g};{b}m{message}\x1b[0m")
    }
}

pub async fn fetch_latest_release() -> Result<GitHubRelease> {
    fetch_release_by_tag("latest").await
}

pub async fn fetch_release_by_tag(tag: &str) -> Result<GitHubRelease> {
    let url = if tag == "latest" {
        format!("{GITHUB_API}/repos/{GITHUB_REPO}/releases/latest")
    } else {
        format!("{GITHUB_API}/repos/{GITHUB_REPO}/releases/tags/{tag}")
    };
    let client = reqwest::Client::builder()
        .build()
        .context("failed to build HTTP client")?;
    let mut req = client
        .get(url)
        .header(reqwest::header::USER_AGENT, user_agent())
        .header(reqwest::header::ACCEPT, "application/vnd.github+json");

    if let Some(token) = github_token() {
        req = req.bearer_auth(token);
    }

    let resp = req.send().await.context("failed to fetch latest release")?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("latest release request failed: HTTP {status}: {body}");
    }

    serde_json::from_str(&body).context("failed to parse latest release response")
}

pub async fn download_asset(asset: GitHubReleaseAsset) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .build()
        .context("failed to build HTTP client")?;
    let mut req = client
        .get(&asset.url)
        .header(reqwest::header::USER_AGENT, user_agent())
        .header(reqwest::header::ACCEPT, "application/octet-stream");

    if let Some(token) = github_token() {
        req = req.bearer_auth(token);
    }

    let resp = req
        .send()
        .await
        .with_context(|| format!("failed to download asset {}", asset.name))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("asset download failed: HTTP {status}: {body}");
    }

    let total = resp.content_length().unwrap_or(0);
    let mut bytes = Vec::with_capacity(total as usize);
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed to read download chunk")?;
        downloaded += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);
        if total > 0 {
            let pct = (downloaded * 100) / total;
            eprint!("\rDownloading... {pct}% ({downloaded}/{total} bytes)");
        } else {
            eprint!("\rDownloading... {downloaded} bytes");
        }
    }
    eprintln!();

    Ok(bytes)
}

pub fn extract_binary(archive: &[u8]) -> Result<Vec<u8>> {
    let decoder = GzDecoder::new(Cursor::new(archive));
    let mut tar = tar::Archive::new(decoder);

    for entry in tar
        .entries()
        .context("failed to read release archive entries")?
    {
        let mut entry = entry.context("failed to read archive entry")?;
        let path = entry.path().context("failed to read archive entry path")?;
        if path == Path::new(BINARY_NAME) || path == Path::new("bin").join(BINARY_NAME) {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes)
                .context("failed to read extracted binary")?;
            return Ok(bytes);
        }
    }

    anyhow::bail!("binary {BINARY_NAME} not found in archive")
}

pub fn install_binary(current_exe: &Path, binary: &[u8]) -> Result<()> {
    let parent = current_exe
        .parent()
        .context("current executable has no parent directory")?;
    let file_name = current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .context("current executable name is not valid UTF-8")?;
    let temp_path = parent.join(format!(".{file_name}.download-{}", ulid::Ulid::new()));

    fs::write(&temp_path, binary)
        .with_context(|| format!("failed to write temporary binary {}", temp_path.display()))?;
    copy_permissions(current_exe, &temp_path)?;
    fs::rename(&temp_path, current_exe).with_context(|| {
        format!(
            "failed to replace {} with {}",
            current_exe.display(),
            temp_path.display()
        )
    })?;
    Ok(())
}

fn copy_permissions(source: &Path, target: &Path) -> Result<()> {
    let metadata =
        fs::metadata(source).with_context(|| format!("failed to stat {}", source.display()))?;
    let permissions = metadata.permissions();
    fs::set_permissions(target, permissions)
        .with_context(|| format!("failed to set permissions on {}", target.display()))?;
    Ok(())
}

pub fn select_asset(release: &GitHubRelease, target: &str) -> Option<GitHubReleaseAsset> {
    let exact_name = format!("{}-{}-{}.tar.gz", BINARY_NAME, release.tag_name, target);
    release
        .assets
        .iter()
        .find(|asset| asset.name == exact_name)
        .or_else(|| {
            release
                .assets
                .iter()
                .find(|asset| asset.name.contains(target) && asset.name.ends_with(".tar.gz"))
        })
        .cloned()
}

pub fn current_release_tag() -> String {
    if !crate::version::BENDCLAW_GIT_TAG.is_empty() && crate::version::BENDCLAW_GIT_TAG != "unknown"
    {
        crate::version::BENDCLAW_GIT_TAG.to_string()
    } else {
        format!("v{}", crate::version::BENDCLAW_VERSION)
    }
}

pub fn supported_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        (os, arch) => anyhow::bail!("self-update is not supported for platform {arch}-{os}"),
    }
}

fn github_token() -> Option<String> {
    std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn user_agent() -> String {
    let ver = current_release_tag();
    let ver = ver.trim_start_matches('v');
    format!("{BINARY_NAME}/{ver}")
}

pub fn tags_match(current: &str, latest: &str) -> bool {
    current == latest || current.trim_start_matches('v') == latest.trim_start_matches('v')
}

fn stdout_supports_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }

    if std::env::var("CLICOLOR_FORCE")
        .map(|value| value != "0")
        .unwrap_or(false)
    {
        return true;
    }

    if std::env::var("CLICOLOR")
        .map(|value| value == "0")
        .unwrap_or(false)
    {
        return false;
    }

    std::io::stdout().is_terminal()
}
