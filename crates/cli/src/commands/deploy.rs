use anyhow::{Context, Result};
use release_kit_core::config::parse_album_toml;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use super::build::build_static_site;

/// Global configuration for deployments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    pub cloudflare: CloudflareConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareConfig {
    pub api_token: String,
    pub account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_domain: Option<String>,
}

/// Get path to global config file
fn config_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    let config_dir = PathBuf::from(home).join(".release-kit");
    fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("config.toml"))
}

/// Load global config
fn load_config() -> Result<Option<GlobalConfig>> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&path)
        .context("Failed to read config file")?;
    let config: GlobalConfig = toml::from_str(&contents)
        .context("Failed to parse config file")?;
    Ok(Some(config))
}

/// Save global config
fn save_config(config: &GlobalConfig) -> Result<()> {
    let path = config_path()?;
    let contents = toml::to_string_pretty(config)
        .context("Failed to serialize config")?;
    fs::write(&path, contents)
        .context("Failed to write config file")?;
    println!("✅ Configuration saved to: {}", path.display());
    Ok(())
}

/// Derive project name from album metadata
/// Format: {artist-slug}-{album-slug}
/// Example: "Artist Name" + "My Album" -> "artist-name-my-album"
fn derive_project_name(artist: &str, album: &str) -> String {
    let slugify = |s: &str| -> String {
        s.to_lowercase()
            .chars()
            .map(|c| {
                // Only keep ASCII alphanumeric for URL safety
                if c.is_ascii_alphanumeric() {
                    c
                } else if c.is_whitespace() || c == '-' || c == '_' {
                    '-'
                } else {
                    // Drop special characters and unicode
                    '\0'
                }
            })
            .filter(|&c| c != '\0')
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    };

    format!("{}-{}", slugify(artist), slugify(album))
}

// ============================================================================
// Cloudflare API Client
// ============================================================================

/// Cloudflare API client
struct CloudflareClient {
    client: reqwest::Client,
    account_id: String,
}

/// Cloudflare API response wrapper
#[derive(Debug, Deserialize)]
struct CloudflareResponse<T> {
    success: bool,
    errors: Vec<CloudflareError>,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
struct CloudflareError {
    code: i32,
    message: String,
}

/// Pages project info from API
#[derive(Debug, Deserialize, Serialize)]
struct PagesProject {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    subdomain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    domains: Option<Vec<String>>,
    created_on: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    production_branch: Option<String>,
}

/// DNS Zone info
#[derive(Debug, Deserialize)]
struct DnsZone {
    id: String,
    name: String,
}

/// DNS Record
#[derive(Debug, Deserialize, Serialize)]
struct DnsRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(rename = "type")]
    record_type: String,
    name: String,
    content: String,
    proxied: bool,
}

impl CloudflareClient {
    /// Create new Cloudflare API client
    fn new(api_token: &str, account_id: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_token))?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self {
            client,
            account_id: account_id.to_string(),
        })
    }

    /// Get Pages project by name
    async fn get_pages_project(&self, project_name: &str) -> Result<Option<PagesProject>> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/pages/projects/{}",
            self.account_id, project_name
        );

        let response = self.client.get(&url).send().await?;

        if response.status() == 404 {
            return Ok(None);
        }

        let cf_response: CloudflareResponse<PagesProject> = response.json().await?;

        if !cf_response.success {
            if let Some(error) = cf_response.errors.first() {
                anyhow::bail!("Cloudflare API error: {}", error.message);
            }
            anyhow::bail!("Unknown Cloudflare API error");
        }

        Ok(cf_response.result)
    }

    /// Create Pages project
    async fn create_pages_project(&self, project_name: &str) -> Result<PagesProject> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/pages/projects",
            self.account_id
        );

        #[derive(Serialize)]
        struct CreateProjectRequest {
            name: String,
            production_branch: String,
        }

        let request = CreateProjectRequest {
            name: project_name.to_string(),
            production_branch: "main".to_string(),
        };

        let response = self.client.post(&url).json(&request).send().await?;
        let cf_response: CloudflareResponse<PagesProject> = response.json().await?;

        if !cf_response.success {
            if let Some(error) = cf_response.errors.first() {
                anyhow::bail!("Cloudflare API error: {}", error.message);
            }
            anyhow::bail!("Unknown Cloudflare API error");
        }

        cf_response
            .result
            .context("No project returned from API")
    }

    /// Upload static site files to Pages project (Direct Upload)
    async fn upload_deployment(&self, project_name: &str, build_dir: &Path) -> Result<String> {
        // Create zip file of build directory
        let zip_path = create_deployment_zip(build_dir)?;

        // Upload via Cloudflare Pages Direct Upload API
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/pages/projects/{}/deployments",
            self.account_id, project_name
        );

        // Read the zip file
        let zip_bytes = std::fs::read(&zip_path)
            .context("Failed to read deployment zip")?;

        // Create multipart form
        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(zip_bytes)
                    .file_name("deployment.zip")
                    .mime_str("application/zip")?,
            );

        let response = self.client
            .post(&url)
            .multipart(form)
            .send()
            .await?;

        let status = response.status();
        let response_text = response.text().await?;

        if !status.is_success() {
            anyhow::bail!("Upload failed ({}): {}", status, response_text);
        }

        // Parse response to get deployment URL
        let cf_response: serde_json::Value = serde_json::from_str(&response_text)?;

        let deployment_url = cf_response
            .get("result")
            .and_then(|r| r.get("url"))
            .and_then(|u| u.as_str())
            .unwrap_or(&format!("https://{}.pages.dev", project_name))
            .to_string();

        Ok(deployment_url)
    }

    /// Delete Pages project
    async fn delete_pages_project(&self, project_name: &str) -> Result<()> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/pages/projects/{}",
            self.account_id, project_name
        );

        let response = self.client.delete(&url).send().await?;
        let cf_response: CloudflareResponse<serde_json::Value> = response.json().await?;

        if !cf_response.success {
            if let Some(error) = cf_response.errors.first() {
                anyhow::bail!("Cloudflare API error: {}", error.message);
            }
            anyhow::bail!("Unknown Cloudflare API error");
        }

        Ok(())
    }

    /// Get DNS zone by domain name
    async fn get_dns_zone(&self, domain: &str) -> Result<Option<DnsZone>> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones?name={}",
            domain
        );

        let response = self.client.get(&url).send().await?;
        let cf_response: CloudflareResponse<Vec<DnsZone>> = response.json().await?;

        if !cf_response.success {
            if let Some(error) = cf_response.errors.first() {
                anyhow::bail!("Cloudflare API error: {}", error.message);
            }
            anyhow::bail!("Unknown Cloudflare API error");
        }

        Ok(cf_response.result.and_then(|mut zones| zones.pop()))
    }

    /// Create DNS CNAME record
    async fn create_dns_record(
        &self,
        zone_id: &str,
        name: &str,
        target: &str,
    ) -> Result<DnsRecord> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
            zone_id
        );

        let record = DnsRecord {
            id: None,
            record_type: "CNAME".to_string(),
            name: name.to_string(),
            content: target.to_string(),
            proxied: true, // Enable Cloudflare proxy for HTTPS
        };

        let response = self.client.post(&url).json(&record).send().await?;
        let cf_response: CloudflareResponse<DnsRecord> = response.json().await?;

        if !cf_response.success {
            if let Some(error) = cf_response.errors.first() {
                anyhow::bail!("Cloudflare API error: {}", error.message);
            }
            anyhow::bail!("Unknown Cloudflare API error");
        }

        cf_response
            .result
            .context("No DNS record returned from API")
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a zip file of the build directory for deployment
fn create_deployment_zip(build_dir: &Path) -> Result<PathBuf> {
    let zip_path = std::env::temp_dir().join(format!(
        "release-kit-deploy-{}.zip",
        std::process::id()
    ));

    let file = File::create(&zip_path)
        .context("Failed to create deployment zip file")?;
    let mut zip = ZipWriter::new(file);

    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // Walk the build directory and add all files
    for entry in WalkDir::new(build_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let relative_path = path.strip_prefix(build_dir)
            .context("Failed to get relative path")?;

        // Add file to zip
        zip.start_file(
            relative_path.to_string_lossy().to_string(),
            options,
        )?;

        let mut f = File::open(path)?;
        std::io::copy(&mut f, &mut zip)?;
    }

    zip.finish()?;

    Ok(zip_path)
}

// ============================================================================
// Deploy Commands
// ============================================================================

/// Helper to read user input
fn read_input(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

/// Configure Cloudflare credentials and base domain
pub async fn configure() -> Result<()> {
    println!("🔧 Configuring Cloudflare deployment...\n");

    // Load existing config if any
    let existing = load_config()?;

    println!("📋 You'll need:");
    println!("   1. Cloudflare API Token (with Pages permissions)");
    println!("      Create at: https://dash.cloudflare.com/profile/api-tokens");
    println!("   2. Cloudflare Account ID");
    println!("      Find at: https://dash.cloudflare.com/ (right sidebar)");
    println!("   3. Base Domain (optional - must be on Cloudflare DNS)");
    println!("      Example: mydomain.com");
    println!();

    // Get API token
    let default_token = existing.as_ref()
        .map(|c| c.cloudflare.api_token.as_str())
        .unwrap_or("");
    let api_token = if !default_token.is_empty() {
        let input = read_input(&format!("API Token [current: {}...]: ", &default_token[..10.min(default_token.len())]))?;
        if input.is_empty() {
            default_token.to_string()
        } else {
            input
        }
    } else {
        read_input("API Token: ")?
    };

    if api_token.is_empty() {
        anyhow::bail!("API token is required");
    }

    // Get account ID
    let default_account = existing.as_ref()
        .map(|c| c.cloudflare.account_id.as_str())
        .unwrap_or("");
    let account_id = if !default_account.is_empty() {
        let input = read_input(&format!("Account ID [current: {}]: ", default_account))?;
        if input.is_empty() {
            default_account.to_string()
        } else {
            input
        }
    } else {
        read_input("Account ID: ")?
    };

    if account_id.is_empty() {
        anyhow::bail!("Account ID is required");
    }

    // Get base domain (optional)
    let default_domain = existing.as_ref()
        .and_then(|c| c.cloudflare.base_domain.as_ref())
        .map(|s| s.as_str())
        .unwrap_or("");
    let base_domain_input = if !default_domain.is_empty() {
        let input = read_input(&format!("Base Domain [current: {}] (press Enter to keep, 'none' to remove): ", default_domain))?;
        if input.is_empty() {
            Some(default_domain.to_string())
        } else if input.eq_ignore_ascii_case("none") {
            None
        } else {
            Some(input)
        }
    } else {
        let input = read_input("Base Domain (optional, press Enter to skip): ")?;
        if input.is_empty() {
            None
        } else {
            Some(input)
        }
    };

    // Create config
    let config = GlobalConfig {
        cloudflare: CloudflareConfig {
            api_token,
            account_id,
            base_domain: base_domain_input,
        },
    };

    // Save config
    save_config(&config)?;

    println!();
    println!("✅ Configuration complete!");
    if let Some(domain) = &config.cloudflare.base_domain {
        println!("   Albums will deploy to subdomains of: {}", domain);
        println!("   Example: album-name.{}", domain);
    } else {
        println!("   Albums will deploy to: *.pages.dev");
        println!("   💡 Tip: Add a base domain later with 'release-kit deploy configure'");
    }
    println!();
    println!("🚀 Ready to deploy! Try: release-kit deploy publish <album-path>");

    Ok(())
}

/// Publish album to Cloudflare Pages
pub async fn publish(path: PathBuf, force: bool) -> Result<()> {
    println!("🚀 Publishing album to Cloudflare Pages...\n");

    // Validate and load album config
    let album_toml_path = path.join("album.toml");
    if !album_toml_path.exists() {
        anyhow::bail!(
            "album.toml not found in {}\nRun 'release-kit init {}' first",
            path.display(),
            path.display()
        );
    }

    let album = parse_album_toml(&album_toml_path).context("Failed to parse album.toml")?;
    let project_name = derive_project_name(&album.artist.name, &album.metadata.title);

    // Get subdomain from album config if specified
    let subdomain = album.hosting.cloudflare.subdomain.clone();

    println!("📋 Deployment Plan:");
    println!("   Album: {}", album.metadata.title);
    println!("   Artist: {}", album.artist.name);
    println!("   Project: {}", project_name);
    println!("   Target: Cloudflare Pages (Free Tier)");
    if let Some(ref sub) = subdomain {
        println!("   Subdomain: {}", sub);
    }
    println!();

    // Load global config
    let config = load_config()?.context(
        "No Cloudflare configuration found.\nRun 'release-kit deploy configure' first",
    )?;

    // Check if project exists via API
    println!("🔍 Checking deployment status...");
    let client = CloudflareClient::new(&config.cloudflare.api_token, &config.cloudflare.account_id)?;

    let project_exists = match client.get_pages_project(&project_name).await? {
        Some(_) => {
            println!("   ✓ Project exists - will update");
            true
        }
        None => {
            println!("   ℹ️  Project doesn't exist - will create");
            false
        }
    };
    println!();

    // Confirmation prompt
    if !force {
        print!("❓ Deploy to Cloudflare Pages? (y/N): ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("❌ Deployment cancelled");
            return Ok(());
        }
        println!();
    }

    // Build static site to temp directory
    println!("📦 Building static site...");
    let _temp_dir = TempDir::new().context("Failed to create temporary directory")?;
    let build_dir = _temp_dir.path();
    build_static_site(&path, build_dir, false)?;
    println!("   ✓ Built to: {}", build_dir.display());
    println!();

    // Create project if it doesn't exist
    if !project_exists {
        println!("📝 Creating Cloudflare Pages project...");
        client.create_pages_project(&project_name).await?;
        println!("   ✓ Project created");
        println!();
    }

    // Upload deployment
    println!("☁️  Deploying to Cloudflare...");
    let deployment_url = client.upload_deployment(&project_name, build_dir).await?;
    println!("   ✓ Deployed successfully");
    println!();

    // Set up custom domain if configured
    if let (Some(subdomain), Some(base_domain)) = (&album.hosting.cloudflare.subdomain, &config.cloudflare.base_domain) {
        println!("🌐 Setting up custom domain...");
        let full_domain = format!("{}.{}", subdomain, base_domain);

        // Get DNS zone
        match client.get_dns_zone(base_domain).await? {
            Some(zone) => {
                println!("   ✓ Found DNS zone for {}", base_domain);

                // Create CNAME record
                let target = format!("{}.pages.dev", project_name);
                match client.create_dns_record(&zone.id, &full_domain, &target).await {
                    Ok(_) => {
                        println!("   ✓ Created DNS record: {} → {}", full_domain, target);
                    }
                    Err(e) => {
                        println!("   ⚠️  DNS record creation failed: {}", e);
                        println!("   💡 You may need to create it manually in Cloudflare dashboard");
                    }
                }
            }
            None => {
                println!("   ⚠️  Domain {} not found on Cloudflare", base_domain);
                println!("   💡 Add your domain to Cloudflare DNS first");
            }
        }
        println!();
    }

    println!("✅ Deployment complete!");
    println!("   Live URL: {}", deployment_url);
    if let (Some(subdomain), Some(base_domain)) = (&album.hosting.cloudflare.subdomain, &config.cloudflare.base_domain) {
        println!("   Custom domain: https://{}.{} (DNS propagation may take a few minutes)", subdomain, base_domain);
    }

    Ok(())
}

/// Show deployment status
pub async fn status(path: Option<PathBuf>) -> Result<()> {
    let path = path.unwrap_or_else(|| PathBuf::from("."));

    println!("📊 Checking deployment status...\n");

    // Validate and load album config
    let album_toml_path = path.join("album.toml");
    if !album_toml_path.exists() {
        anyhow::bail!(
            "album.toml not found in {}\nNot an album directory?",
            path.display()
        );
    }

    let album = parse_album_toml(&album_toml_path).context("Failed to parse album.toml")?;
    let project_name = derive_project_name(&album.artist.name, &album.metadata.title);

    println!("📋 Project Information:");
    println!("   Album: {}", album.metadata.title);
    println!("   Artist: {}", album.artist.name);
    println!("   Project: {}", project_name);
    println!();

    // Load global config
    let config = load_config()?.context(
        "No Cloudflare configuration found.\nRun 'release-kit deploy configure' first",
    )?;

    // Query Cloudflare API
    println!("☁️  Cloudflare Pages Status:");
    let client = CloudflareClient::new(&config.cloudflare.api_token, &config.cloudflare.account_id)?;

    match client.get_pages_project(&project_name).await? {
        Some(project) => {
            println!("   ✅ Status: Deployed");
            println!("   Created: {}", project.created_on);
            println!("   URL: https://{}.pages.dev", project_name);

            if let Some(domains) = &project.domains {
                if !domains.is_empty() {
                    println!("   Custom Domains:");
                    for domain in domains {
                        println!("     - https://{}", domain);
                    }
                }
            }
        }
        None => {
            println!("   ❌ Status: Not deployed");
            println!("   Run 'release-kit deploy publish {}' to deploy", path.display());
        }
    }
    println!();

    println!("💰 Usage Information:");
    println!("   Free Tier: 500 builds/month");
    println!("   Builds this month: Check Cloudflare dashboard");

    Ok(())
}

/// Teardown deployment from Cloudflare Pages
pub async fn teardown(path: PathBuf, force: bool) -> Result<()> {
    println!("🗑️  Tearing down Cloudflare Pages deployment...\n");

    // Validate and load album config
    let album_toml_path = path.join("album.toml");
    if !album_toml_path.exists() {
        anyhow::bail!(
            "album.toml not found in {}\nNot an album directory?",
            path.display()
        );
    }

    let album = parse_album_toml(&album_toml_path).context("Failed to parse album.toml")?;
    let project_name = derive_project_name(&album.artist.name, &album.metadata.title);

    println!("⚠️  WARNING: This will permanently delete:");
    println!("   Project: {}", project_name);
    println!("   URL: https://{}.pages.dev", project_name);
    println!("   All deployments and history");
    println!();

    // Load global config
    let config = load_config()?.context(
        "No Cloudflare configuration found.\nRun 'release-kit deploy configure' first",
    )?;

    // Check if project exists via API
    println!("🔍 Checking if project exists...");
    let client = CloudflareClient::new(&config.cloudflare.api_token, &config.cloudflare.account_id)?;

    match client.get_pages_project(&project_name).await? {
        Some(_) => {
            println!("   ✓ Project found");
        }
        None => {
            println!("   ℹ️  Project not found - nothing to delete");
            return Ok(());
        }
    }
    println!();

    // Confirmation prompt
    if !force {
        println!("⚠️  Type the project name to confirm deletion:");
        print!("   > ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if input.trim() != project_name {
            println!("❌ Project name doesn't match. Teardown cancelled.");
            return Ok(());
        }
    }

    println!("🗑️  Deleting project from Cloudflare...");
    client.delete_pages_project(&project_name).await?;
    println!("   ✓ Deleted from Cloudflare Pages");
    println!();

    println!("✅ Teardown complete!");
    println!("   Project {} has been deleted", project_name);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_project_name_basic() {
        assert_eq!(
            derive_project_name("Artist Name", "My Album"),
            "artist-name-my-album"
        );
    }

    #[test]
    fn test_derive_project_name_special_chars() {
        assert_eq!(
            derive_project_name("DJ K!ool", "Beats & Bass"),
            "dj-kool-beats-bass"
        );
    }

    #[test]
    fn test_derive_project_name_unicode() {
        assert_eq!(
            derive_project_name("Café Tacvba", "Ré Album"),
            "caf-tacvba-r-album"
        );
    }

    #[test]
    fn test_derive_project_name_multiple_spaces() {
        assert_eq!(
            derive_project_name("The   Cool  Band", "Super    Album"),
            "the-cool-band-super-album"
        );
    }

    #[test]
    fn test_derive_project_name_hyphens() {
        assert_eq!(
            derive_project_name("Jay-Z", "The-Album"),
            "jay-z-the-album"
        );
    }

    #[test]
    fn test_derive_project_name_numbers() {
        assert_eq!(
            derive_project_name("Blink 182", "Album 2023"),
            "blink-182-album-2023"
        );
    }
}
