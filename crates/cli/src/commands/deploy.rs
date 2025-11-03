use anyhow::{Context, Result};
use release_kit_core::config::parse_album_toml;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use s3::Bucket as S3Bucket;
use s3::Region as S3Region;
use s3::creds::Credentials as S3Credentials;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;

use super::build::build_static_site;

// Constants
const DEFAULT_BRANCH: &str = "main";
const DNS_RECORD_TYPE: &str = "CNAME";
const HTTP_TIMEOUT_SECS: u64 = 300; // 5 minutes for large uploads

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
    /// R2 Access Key ID (S3-compatible credentials) - REQUIRED
    pub r2_access_key_id: String,
    /// R2 Secret Access Key (S3-compatible credentials) - REQUIRED
    pub r2_secret_access_key: String,
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
    let contents = fs::read_to_string(&path).context("Failed to read config file")?;
    let config: GlobalConfig = toml::from_str(&contents).context("Failed to parse config file")?;
    Ok(Some(config))
}

/// Save global config with secure permissions
fn save_config(config: &GlobalConfig) -> Result<()> {
    let path = config_path()?;
    let contents = toml::to_string_pretty(config).context("Failed to serialize config")?;
    fs::write(&path, contents).context("Failed to write config file")?;

    // Set secure file permissions (0600 - owner read/write only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        fs::set_permissions(&path, permissions).context("Failed to set secure file permissions")?;
        println!(
            "✅ Configuration saved to: {} (permissions: 0600)",
            path.display()
        );
    }

    #[cfg(not(unix))]
    {
        println!("✅ Configuration saved to: {}", path.display());
        println!("⚠️  Warning: File permissions not set (non-Unix platform)");
        println!("   Please ensure the config file is only readable by your user");
    }

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
    #[allow(dead_code)]
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
    _name: String,
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

/// R2 Bucket info
#[derive(Debug, Deserialize, Serialize)]
struct R2Bucket {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    creation_date: Option<String>,
}

/// R2 Custom Domain
#[derive(Debug, Deserialize, Serialize)]
struct R2CustomDomain {
    domain: String,
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
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
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
            production_branch: DEFAULT_BRANCH.to_string(),
        };

        let response = self.client.post(&url).json(&request).send().await?;
        let cf_response: CloudflareResponse<PagesProject> = response.json().await?;

        if !cf_response.success {
            if let Some(error) = cf_response.errors.first() {
                anyhow::bail!("Cloudflare API error: {}", error.message);
            }
            anyhow::bail!("Unknown Cloudflare API error");
        }

        cf_response.result.context("No project returned from API")
    }

    /// Upload static site files to Pages project (Direct Upload)
    async fn upload_deployment(&self, project_name: &str, build_dir: &Path) -> Result<String> {
        use std::collections::HashMap;
        use walkdir::WalkDir;

        // Build manifest of all files with their hashes
        let mut manifest = HashMap::new();
        let mut form = reqwest::multipart::Form::new();

        for entry in WalkDir::new(build_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            let relative_path = path
                .strip_prefix(build_dir)
                .context("Failed to get relative path")?
                .to_string_lossy()
                .replace('\\', "/"); // Normalize path separators

            // Read file and calculate hash
            let file_bytes = std::fs::read(path)
                .with_context(|| format!("Failed to read file: {}", path.display()))?;

            // Use a simple hash for the manifest (Cloudflare may not strictly validate this)
            let hash = format!("{:x}", file_bytes.len()); // Simple approach: use file size as hash

            manifest.insert(relative_path.clone(), hash);

            // Add file to multipart form
            let mime_type = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();

            form = form.part(
                relative_path.clone(),
                reqwest::multipart::Part::bytes(file_bytes)
                    .file_name(relative_path.clone())
                    .mime_str(&mime_type)?,
            );
        }

        // Add manifest as JSON field
        let manifest_json = serde_json::to_string(&manifest)?;
        form = form.text("manifest", manifest_json);

        // Upload via Cloudflare Pages Direct Upload API
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/pages/projects/{}/deployments",
            self.account_id, project_name
        );

        let response = self.client.post(&url).multipart(form).send().await?;

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
        let url = format!("https://api.cloudflare.com/client/v4/zones?name={}", domain);

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
            record_type: DNS_RECORD_TYPE.to_string(),
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

    /// Get R2 bucket by name
    async fn get_r2_bucket(&self, bucket_name: &str) -> Result<Option<R2Bucket>> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/r2/buckets/{}",
            self.account_id, bucket_name
        );

        let response = self.client.get(&url).send().await?;

        if response.status() == 404 {
            return Ok(None);
        }

        let cf_response: CloudflareResponse<R2Bucket> = response.json().await?;

        if !cf_response.success {
            if let Some(error) = cf_response.errors.first() {
                anyhow::bail!("Cloudflare API error: {}", error.message);
            }
            anyhow::bail!("Unknown Cloudflare API error");
        }

        Ok(cf_response.result)
    }

    /// Create R2 bucket
    async fn create_r2_bucket(&self, bucket_name: &str) -> Result<R2Bucket> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/r2/buckets",
            self.account_id
        );

        #[derive(Serialize)]
        struct CreateBucketRequest {
            name: String,
        }

        let request = CreateBucketRequest {
            name: bucket_name.to_string(),
        };

        let response = self.client.post(&url).json(&request).send().await?;
        let cf_response: CloudflareResponse<R2Bucket> = response.json().await?;

        if !cf_response.success {
            if let Some(error) = cf_response.errors.first() {
                anyhow::bail!("Cloudflare API error: {}", error.message);
            }
            anyhow::bail!("Unknown Cloudflare API error");
        }

        cf_response.result.context("No bucket returned from API")
    }

    /// Empty R2 bucket by deleting all objects
    async fn empty_r2_bucket(
        &self,
        bucket_name: &str,
        r2_access_key_id: &str,
        r2_secret_access_key: &str,
    ) -> Result<()> {
        // Create rust-s3 bucket for R2
        let credentials = S3Credentials::new(
            Some(r2_access_key_id),
            Some(r2_secret_access_key),
            None,
            None,
            None,
        )?;

        let region = S3Region::R2 {
            account_id: self.account_id.clone(),
        };

        let bucket = S3Bucket::new(bucket_name, region, credentials)?.with_path_style();

        // List all objects in the bucket
        println!("      Listing bucket: {}", bucket_name);
        println!(
            "      Endpoint: https://{}.r2.cloudflarestorage.com",
            self.account_id
        );

        // List all completed objects
        println!("      Listing completed objects...");
        let list_results = bucket.list("".to_string(), None).await?;

        let mut all_keys = Vec::new();

        // Collect all object keys
        for (idx, list) in list_results.iter().enumerate() {
            println!(
                "      Page {}: {} objects, {} common prefixes, truncated: {}",
                idx,
                list.contents.len(),
                list.common_prefixes.as_ref().map(|p| p.len()).unwrap_or(0),
                list.is_truncated
            );

            for obj in &list.contents {
                all_keys.push(obj.key.clone());
            }

            // Also check common prefixes (directories)
            if let Some(prefixes) = &list.common_prefixes {
                for prefix in prefixes {
                    println!("      Found prefix: {}", prefix.prefix);
                    // List objects under this prefix
                    let prefix_results = bucket.list(prefix.prefix.clone(), None).await?;
                    for prefix_list in prefix_results {
                        for obj in &prefix_list.contents {
                            all_keys.push(obj.key.clone());
                        }
                    }
                }
            }
        }

        let total_objects = all_keys.len();
        let mut deleted_objects = 0;

        // Delete all objects
        for key in all_keys {
            println!("      Deleting: {}", key);
            bucket
                .delete_object(&key)
                .await
                .with_context(|| format!("Failed to delete object: {}", key))?;
            deleted_objects += 1;
        }

        if total_objects > 0 {
            println!("      ✓ Deleted {} objects", deleted_objects);
        } else {
            println!("      ⚠️  No completed objects found");
        }

        // List and abort incomplete multipart uploads
        println!("      Checking for incomplete multipart uploads...");
        let multipart_results = bucket.list_multiparts_uploads(None, None).await?;

        let mut total_uploads = 0;
        let mut aborted_uploads = 0;

        for upload_list in multipart_results {
            total_uploads += upload_list.uploads.len();
            for upload in &upload_list.uploads {
                println!(
                    "      Aborting multipart upload: {} ({})",
                    upload.key, upload.id
                );
                match bucket.abort_upload(&upload.key, &upload.id).await {
                    Ok(_) => {
                        aborted_uploads += 1;
                    }
                    Err(e) => {
                        eprintln!("      ⚠️  Failed to abort upload {}: {}", upload.key, e);
                    }
                }
            }
        }

        if total_uploads > 0 {
            println!("      ✓ Aborted {} multipart uploads", aborted_uploads);
        } else {
            println!("      ✓ No incomplete uploads found");
        }

        Ok(())
    }

    /// Delete R2 bucket
    async fn delete_r2_bucket(&self, bucket_name: &str) -> Result<()> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/r2/buckets/{}",
            self.account_id, bucket_name
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

    /// Configure R2 bucket for public access with CORS
    async fn configure_r2_public_access(&self, bucket_name: &str) -> Result<()> {
        // Set CORS policy to allow browser access
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/r2/buckets/{}/cors",
            self.account_id, bucket_name
        );

        #[derive(Serialize)]
        struct CorsRule {
            allowed_origins: Vec<String>,
            allowed_methods: Vec<String>,
            allowed_headers: Vec<String>,
            max_age_seconds: u32,
        }

        #[derive(Serialize)]
        struct CorsConfig {
            cors_rules: Vec<CorsRule>,
        }

        let config = CorsConfig {
            cors_rules: vec![CorsRule {
                allowed_origins: vec!["*".to_string()],
                allowed_methods: vec!["GET".to_string(), "HEAD".to_string()],
                allowed_headers: vec!["*".to_string()],
                max_age_seconds: 3600,
            }],
        };

        let response = self.client.put(&url).json(&config).send().await?;
        let cf_response: CloudflareResponse<serde_json::Value> = response.json().await?;

        if !cf_response.success {
            if let Some(error) = cf_response.errors.first() {
                anyhow::bail!("Cloudflare API error: {}", error.message);
            }
            anyhow::bail!("Unknown Cloudflare API error");
        }

        Ok(())
    }

    /// Add custom domain to R2 bucket
    async fn add_r2_custom_domain(&self, bucket_name: &str, domain: &str) -> Result<()> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/r2/buckets/{}/domains",
            self.account_id, bucket_name
        );

        let request = R2CustomDomain {
            domain: domain.to_string(),
        };

        let response = self.client.post(&url).json(&request).send().await?;
        let cf_response: CloudflareResponse<R2CustomDomain> = response.json().await?;

        if !cf_response.success {
            if let Some(error) = cf_response.errors.first() {
                anyhow::bail!("Cloudflare API error: {}", error.message);
            }
            anyhow::bail!("Unknown Cloudflare API error");
        }

        Ok(())
    }
}

// ============================================================================
// Helper Functions
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

/// Validate Cloudflare API token format
fn validate_api_token(token: &str) -> Result<()> {
    if token.is_empty() {
        anyhow::bail!("API token cannot be empty");
    }
    if token.len() < 20 {
        anyhow::bail!("API token appears too short (expected 40+ characters)");
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        anyhow::bail!("API token contains invalid characters");
    }
    Ok(())
}

/// Validate Cloudflare account ID format
fn validate_account_id(account_id: &str) -> Result<()> {
    if account_id.is_empty() {
        anyhow::bail!("Account ID cannot be empty");
    }
    // Account IDs are 32-character hex strings
    if account_id.len() != 32 {
        anyhow::bail!("Account ID must be exactly 32 characters");
    }
    if !account_id.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("Account ID must be hexadecimal (0-9, a-f)");
    }
    Ok(())
}

/// Validate domain format
fn validate_domain(domain: &str) -> Result<()> {
    if domain.is_empty() {
        anyhow::bail!("Domain cannot be empty");
    }

    // Basic domain validation
    if !domain.contains('.') {
        anyhow::bail!("Domain must contain at least one dot (e.g., example.com)");
    }

    if domain.starts_with('.') || domain.ends_with('.') {
        anyhow::bail!("Domain cannot start or end with a dot");
    }

    if domain.starts_with('-') || domain.ends_with('-') {
        anyhow::bail!("Domain cannot start or end with a hyphen");
    }

    // Check for invalid characters
    if !domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        anyhow::bail!("Domain contains invalid characters (only a-z, 0-9, '.', '-' allowed)");
    }

    // Domain labels (parts between dots) validation
    for label in domain.split('.') {
        if label.is_empty() {
            anyhow::bail!("Domain cannot have consecutive dots");
        }
        if label.len() > 63 {
            anyhow::bail!("Domain label '{}' is too long (max 63 characters)", label);
        }
        if label.starts_with('-') || label.ends_with('-') {
            anyhow::bail!("Domain label '{}' cannot start or end with hyphen", label);
        }
    }

    Ok(())
}

/// Validate R2 access key format
fn validate_r2_access_key(key: &str) -> Result<()> {
    if key.is_empty() {
        anyhow::bail!("R2 access key cannot be empty");
    }
    if key.len() < 10 {
        anyhow::bail!("R2 access key appears too short");
    }
    if !key.chars().all(|c| c.is_ascii_alphanumeric()) {
        anyhow::bail!("R2 access key should only contain alphanumeric characters");
    }
    Ok(())
}

/// Configure Cloudflare credentials and base domain
pub async fn configure() -> Result<()> {
    println!("🔧 Configuring Cloudflare deployment...\n");

    // Load existing config if any
    let existing = load_config()?;

    println!("📋 You'll need:");
    println!("   1. Cloudflare API Token (with Pages + R2 permissions)");
    println!("      Create at: https://dash.cloudflare.com/profile/api-tokens");
    println!("   2. Cloudflare Account ID");
    println!("      Find at: https://dash.cloudflare.com/ (right sidebar)");
    println!("   3. R2 Access Key ID & Secret (for audio storage)");
    println!("      Create at: https://dash.cloudflare.com/ → R2 → Manage R2 API Tokens");
    println!("   4. Base Domain (optional - must be on Cloudflare DNS)");
    println!("      Example: mydomain.com");
    println!();

    // Get API token
    let default_token = existing
        .as_ref()
        .map(|c| c.cloudflare.api_token.as_str())
        .unwrap_or("");
    let api_token = if !default_token.is_empty() {
        let input = read_input(&format!(
            "API Token [current: {}...]: ",
            &default_token[..10.min(default_token.len())]
        ))?;
        if input.is_empty() {
            default_token.to_string()
        } else {
            input
        }
    } else {
        read_input("API Token: ")?
    };

    // Validate API token
    validate_api_token(&api_token).context("Invalid API token format - please check your token")?;

    // Get account ID
    let default_account = existing
        .as_ref()
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

    // Validate account ID
    validate_account_id(&account_id)
        .context("Invalid account ID format - should be 32-character hexadecimal")?;

    // Get R2 Access Key ID (REQUIRED)
    let default_r2_key = existing
        .as_ref()
        .map(|c| c.cloudflare.r2_access_key_id.as_str())
        .unwrap_or("");
    let r2_access_key_id = if !default_r2_key.is_empty() {
        let input = read_input(&format!(
            "R2 Access Key ID [current: {}...]: ",
            &default_r2_key[..10.min(default_r2_key.len())]
        ))?;
        if input.is_empty() {
            default_r2_key.to_string()
        } else {
            validate_r2_access_key(&input).context("Invalid R2 access key format")?;
            input
        }
    } else {
        let input = read_input("R2 Access Key ID: ")?;
        if input.is_empty() {
            anyhow::bail!("R2 Access Key ID is required for audio storage");
        }
        validate_r2_access_key(&input).context("Invalid R2 access key format")?;
        input
    };

    // Get R2 Secret Access Key (REQUIRED)
    let default_r2_secret = existing
        .as_ref()
        .map(|c| c.cloudflare.r2_secret_access_key.as_str())
        .unwrap_or("");
    let r2_secret_access_key = if !default_r2_secret.is_empty() {
        let input = read_input(&format!(
            "R2 Secret Access Key [current: {}...]: ",
            &default_r2_secret[..10.min(default_r2_secret.len())]
        ))?;
        if input.is_empty() {
            default_r2_secret.to_string()
        } else {
            input
        }
    } else {
        let input = read_input("R2 Secret Access Key: ")?;
        if input.is_empty() {
            anyhow::bail!("R2 Secret Access Key is required for audio storage");
        }
        input
    };

    // Get base domain (optional)
    let default_domain = existing
        .as_ref()
        .and_then(|c| c.cloudflare.base_domain.as_ref())
        .map(|s| s.as_str())
        .unwrap_or("");
    let base_domain_input = if !default_domain.is_empty() {
        let input = read_input(&format!(
            "Base Domain [current: {}] (press Enter to keep, 'none' to remove): ",
            default_domain
        ))?;
        if input.is_empty() {
            Some(default_domain.to_string())
        } else if input.eq_ignore_ascii_case("none") {
            None
        } else {
            validate_domain(&input).context("Invalid domain format")?;
            Some(input)
        }
    } else {
        let input = read_input("Base Domain (optional, press Enter to skip): ")?;
        if input.is_empty() {
            None
        } else {
            validate_domain(&input).context("Invalid domain format")?;
            Some(input)
        }
    };

    // Create config
    let config = GlobalConfig {
        cloudflare: CloudflareConfig {
            api_token,
            account_id,
            base_domain: base_domain_input,
            r2_access_key_id,
            r2_secret_access_key,
        },
    };

    // Save config
    save_config(&config)?;

    println!();
    println!("✅ Configuration complete!");
    println!("   ✓ R2 storage configured (audio files will use R2)");

    if let Some(domain) = &config.cloudflare.base_domain {
        println!("   ✓ Base domain: {}", domain);
        println!("   Albums will deploy to subdomains: album-name.{}", domain);
        println!("   Audio will be served from: cdn.{}", domain);
    } else {
        println!("   ⚠️  No base domain configured");
        println!("   Albums will deploy to: *.pages.dev");
        println!("   Audio will be served from: R2 public URL");
        println!("   💡 Tip: Add a base domain with 'release-kit deploy configure'");
    }
    println!();
    println!("🚀 Ready to deploy! Try: release-kit deploy publish <album-path>");

    Ok(())
}

/// Publish album to Cloudflare Pages
pub async fn publish(path: PathBuf, force: bool, concurrency: Option<usize>) -> Result<()> {
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

    // Validate project name is not empty or invalid
    if project_name.is_empty() || project_name == "-" {
        anyhow::bail!(
            "Invalid album/artist names - cannot create project name.\nAlbum: '{}', Artist: '{}'",
            album.metadata.title,
            album.artist.name
        );
    }

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
    let config = load_config()?
        .context("No Cloudflare configuration found.\nRun 'release-kit deploy configure' first")?;

    // Check if project exists via API
    println!("🔍 Checking deployment status...");
    let client =
        CloudflareClient::new(&config.cloudflare.api_token, &config.cloudflare.account_id)?;

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

    // R2 audio storage (always enabled)
    // R2 bucket name: {project-name}-audio
    let bucket_name = format!("{}-audio", project_name);

    println!("📦 Setting up R2 audio storage...");

    // Check if R2 bucket exists
    let bucket_exists = match client.get_r2_bucket(&bucket_name).await? {
        Some(_) => {
            println!("   ✓ R2 bucket exists: {}", bucket_name);
            true
        }
        None => {
            println!("   ℹ️  Creating R2 bucket: {}", bucket_name);
            client.create_r2_bucket(&bucket_name).await?;
            println!("   ✓ R2 bucket created");
            false
        }
    };

    // Upload audio files to R2 with retry logic
    println!("   📤 Uploading audio files to R2...");
    let audio_dir = path.join("audio");
    if !audio_dir.exists() {
        anyhow::bail!("Audio directory not found: {}", audio_dir.display());
    }

    // Create rust-s3 bucket configuration for R2
    let credentials = S3Credentials::new(
        Some(&config.cloudflare.r2_access_key_id),
        Some(&config.cloudflare.r2_secret_access_key),
        None,
        None,
        None,
    )?;

    let region = S3Region::R2 {
        account_id: config.cloudflare.account_id.clone(),
    };

    let bucket = S3Bucket::new(&bucket_name, region, credentials)?.with_path_style();

    // Create semaphore to limit concurrent uploads (default: 3)
    let max_concurrent_uploads = concurrency.unwrap_or(3);
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrent_uploads));
    println!("   ℹ️  Max concurrent uploads: {}", max_concurrent_uploads);

    // Collect upload tasks
    let mut upload_tasks = Vec::new();

    for track in &album.tracks {
        let audio_file = path.join(&track.file);
        if !audio_file.exists() {
            eprintln!(
                "   ⚠️  Warning: Audio file not found: {}",
                audio_file.display()
            );
            continue;
        }

        let filename = audio_file
            .file_name()
            .context("Invalid audio filename")?
            .to_str()
            .context("Invalid UTF-8 in filename")?
            .to_string();

        let r2_key = format!("audio/{}", filename);

        // Clone data needed for async task
        let audio_file_clone = audio_file.clone();
        let bucket_clone = bucket.clone();
        let semaphore_clone = semaphore.clone();

        // Spawn upload task with retry logic and concurrency limiting
        let task = tokio::spawn(async move {
            // Acquire semaphore permit (limits to 3 concurrent uploads)
            let _permit = semaphore_clone.acquire().await.unwrap();

            let content_type = match audio_file_clone.extension().and_then(|e| e.to_str()) {
                Some("flac") => "audio/flac",
                Some("mp3") => "audio/mpeg",
                Some("wav") => "audio/wav",
                Some("ogg") => "audio/ogg",
                _ => "application/octet-stream",
            };

            // Read file into memory (for both small and large files)
            let file_contents = tokio::fs::read(&audio_file_clone)
                .await
                .context("Failed to read file for upload")?;

            // Retry logic: 5 attempts with exponential backoff
            let mut last_error = None;
            for attempt in 1..=5 {
                let result = bucket_clone
                    .put_object_with_content_type(&r2_key, &file_contents, content_type)
                    .await
                    .map(|_| ());

                match result {
                    Ok(_) => {
                        return Ok::<String, anyhow::Error>(filename.clone());
                    }
                    Err(e) => {
                        last_error = Some(e);
                        if attempt < 5 {
                            // Exponential backoff: 1s, 2s, 3s, 4s
                            tokio::time::sleep(Duration::from_secs(attempt)).await;
                        }
                    }
                }
            }

            Err(anyhow::anyhow!(
                "{}: Failed after 5 attempts - {}",
                filename,
                last_error.unwrap()
            ))
        });

        upload_tasks.push(task);
    }

    // Wait for all uploads to complete
    let mut successful_uploads = 0;
    let mut failed_uploads = Vec::new();

    for task in upload_tasks {
        match task.await {
            Ok(Ok(filename)) => {
                successful_uploads += 1;
                println!("      ✓ {}", filename);
            }
            Ok(Err(e)) => {
                // Show full error chain
                failed_uploads.push(format!("{:#}", e));
            }
            Err(e) => {
                failed_uploads.push(format!("Task panic: {}", e));
            }
        }
    }

    if !failed_uploads.is_empty() {
        eprintln!("   ⚠️  Some uploads failed:");
        for error in &failed_uploads {
            eprintln!("      - {}", error);
        }
        anyhow::bail!("{} upload(s) failed", failed_uploads.len());
    }

    println!("   ✓ Uploaded {} audio files", successful_uploads);

    // Configure CORS if bucket was just created (optional - R2 buckets are public by default)
    if !bucket_exists {
        println!("   🔧 Configuring R2 public access...");
        match client.configure_r2_public_access(&bucket_name).await {
            Ok(_) => {
                println!("   ✓ Public access configured");
            }
            Err(e) => {
                println!(
                    "   ⚠️  CORS configuration failed (bucket is still publicly accessible): {}",
                    e
                );
            }
        }
    }

    // Verify bucket is accessible with R2 credentials
    println!("   🔍 Verifying R2 bucket accessibility...");
    match client.get_r2_bucket(&bucket_name).await {
        Ok(Some(_)) => {
            println!("   ✓ R2 bucket verified accessible");
        }
        Ok(None) => {
            anyhow::bail!(
                "R2 bucket '{}' not found after creation - this shouldn't happen",
                bucket_name
            );
        }
        Err(e) => {
            anyhow::bail!(
                "Failed to verify R2 bucket accessibility: {}\n\
                     Please check your R2 credentials and permissions.",
                e
            );
        }
    }

    // Set up custom domain for R2 if base domain is configured
    let cdn_url = if let Some(base_domain) = &config.cloudflare.base_domain {
        let cdn_domain = format!("{}-audio.{}", project_name, base_domain);
        println!("   🌐 Setting up custom domain: {}", cdn_domain);

        // Add custom domain to R2 bucket
        match client.add_r2_custom_domain(&bucket_name, &cdn_domain).await {
            Ok(_) => {
                println!("   ✓ Custom domain configured");

                // Also need to create DNS record pointing to R2
                if let Some(zone) = client.get_dns_zone(base_domain).await? {
                    let r2_target =
                        format!("{}.r2.cloudflarestorage.com", config.cloudflare.account_id);
                    match client
                        .create_dns_record(&zone.id, &cdn_domain, &r2_target)
                        .await
                    {
                        Ok(_) => {
                            println!("   ✓ DNS record created: {} → {}", cdn_domain, r2_target);
                        }
                        Err(e) => {
                            println!("   ⚠️  DNS record creation failed: {}", e);
                            println!("   💡 You may need to create it manually");
                        }
                    }
                }

                format!("https://{}", cdn_domain)
            }
            Err(e) => {
                println!("   ⚠️  Custom domain setup failed: {}", e);
                // Fall back to default R2 public URL
                format!("https://pub-{}.r2.dev", config.cloudflare.account_id)
            }
        }
    } else {
        // Use default R2 public URL
        format!("https://pub-{}.r2.dev", config.cloudflare.account_id)
    };

    println!("   ✓ Audio will be served from: {}", cdn_url);
    println!();

    // Build static site to temp directory (without audio - using R2)
    println!("📦 Building static site...");
    let _temp_dir = TempDir::new().context("Failed to create temporary directory")?;
    let build_dir = _temp_dir.path();
    build_static_site(&path, build_dir, false, Some(&cdn_url))?;
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
    if let (Some(subdomain), Some(base_domain)) = (
        &album.hosting.cloudflare.subdomain,
        &config.cloudflare.base_domain,
    ) {
        println!("🌐 Setting up custom domain...");
        let full_domain = format!("{}.{}", subdomain, base_domain);

        // Get DNS zone
        match client.get_dns_zone(base_domain).await? {
            Some(zone) => {
                println!("   ✓ Found DNS zone for {}", base_domain);

                // Create CNAME record
                let target = format!("{}.pages.dev", project_name);
                match client
                    .create_dns_record(&zone.id, &full_domain, &target)
                    .await
                {
                    Ok(_) => {
                        println!("   ✓ Created DNS record: {} → {}", full_domain, target);
                    }
                    Err(e) => {
                        println!("   ⚠️  DNS record creation failed: {}", e);
                        println!(
                            "   💡 You may need to create it manually in Cloudflare dashboard"
                        );
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
    if let (Some(subdomain), Some(base_domain)) = (
        &album.hosting.cloudflare.subdomain,
        &config.cloudflare.base_domain,
    ) {
        println!(
            "   Custom domain: https://{}.{} (DNS propagation may take a few minutes)",
            subdomain, base_domain
        );
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

    // Validate project name is not empty or invalid
    if project_name.is_empty() || project_name == "-" {
        anyhow::bail!(
            "Invalid album/artist names - cannot derive project name.\nAlbum: '{}', Artist: '{}'",
            album.metadata.title,
            album.artist.name
        );
    }

    println!("📋 Project Information:");
    println!("   Album: {}", album.metadata.title);
    println!("   Artist: {}", album.artist.name);
    println!("   Project: {}", project_name);
    println!();

    // Load global config
    let config = load_config()?
        .context("No Cloudflare configuration found.\nRun 'release-kit deploy configure' first")?;

    // Query Cloudflare API
    println!("☁️  Cloudflare Pages Status:");
    let client =
        CloudflareClient::new(&config.cloudflare.api_token, &config.cloudflare.account_id)?;

    match client.get_pages_project(&project_name).await? {
        Some(project) => {
            println!("   ✅ Status: Deployed");
            println!("   Created: {}", project.created_on);
            println!("   URL: https://{}.pages.dev", project_name);

            if let Some(domains) = &project.domains
                && !domains.is_empty()
            {
                println!("   Custom Domains:");
                for domain in domains {
                    println!("     - https://{}", domain);
                }
            }
        }
        None => {
            println!("   ❌ Status: Not deployed");
            println!(
                "   Run 'release-kit deploy publish {}' to deploy",
                path.display()
            );
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

    // Validate project name is not empty or invalid
    if project_name.is_empty() || project_name == "-" {
        anyhow::bail!(
            "Invalid album/artist names - cannot derive project name.\nAlbum: '{}', Artist: '{}'",
            album.metadata.title,
            album.artist.name
        );
    }

    let bucket_name = format!("{}-audio", project_name);

    println!("⚠️  WARNING: This will permanently delete:");
    println!("   Project: {}", project_name);
    println!("   URL: https://{}.pages.dev", project_name);
    println!("   All deployments and history");
    println!("   R2 Bucket: {} (if exists)", bucket_name);
    println!("   All audio files in R2");
    println!();

    // Load global config
    let config = load_config()?
        .context("No Cloudflare configuration found.\nRun 'release-kit deploy configure' first")?;

    // Check if project and/or R2 bucket exist
    println!("🔍 Checking deployment status...");
    let client =
        CloudflareClient::new(&config.cloudflare.api_token, &config.cloudflare.account_id)?;

    let project_exists = client.get_pages_project(&project_name).await?.is_some();
    let bucket_exists = client.get_r2_bucket(&bucket_name).await?.is_some();

    if project_exists {
        println!("   ✓ Pages project found");
    } else {
        println!("   ℹ️  Pages project not found");
    }

    if bucket_exists {
        println!("   ✓ R2 bucket found");
    } else {
        println!("   ℹ️  R2 bucket not found");
    }

    if !project_exists && !bucket_exists {
        println!();
        println!("ℹ️  Nothing to delete - deployment already cleaned up");
        return Ok(());
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

    // Delete Pages project if it exists
    if project_exists {
        println!("🗑️  Deleting project from Cloudflare...");
        client.delete_pages_project(&project_name).await?;
        println!("   ✓ Deleted from Cloudflare Pages");
    }

    // Delete R2 bucket if it exists
    if bucket_exists {
        println!("   🗑️  Deleting R2 bucket: {}", bucket_name);

        // First, empty the bucket
        match client
            .empty_r2_bucket(
                &bucket_name,
                &config.cloudflare.r2_access_key_id,
                &config.cloudflare.r2_secret_access_key,
            )
            .await
        {
            Ok(_) => {
                println!("   ✓ Emptied R2 bucket");
            }
            Err(e) => {
                println!("   ⚠️  Failed to empty R2 bucket: {}", e);
                println!("   💡 You may need to delete it manually from the Cloudflare dashboard");
                return Ok(());
            }
        }

        // Then delete the empty bucket
        match client.delete_r2_bucket(&bucket_name).await {
            Ok(_) => {
                println!("   ✓ Deleted R2 bucket");
            }
            Err(e) => {
                println!("   ⚠️  Failed to delete R2 bucket: {}", e);
                println!("   💡 You may need to delete it manually from the Cloudflare dashboard");
            }
        }
    }
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
        assert_eq!(derive_project_name("Jay-Z", "The-Album"), "jay-z-the-album");
    }

    #[test]
    fn test_derive_project_name_numbers() {
        assert_eq!(
            derive_project_name("Blink 182", "Album 2023"),
            "blink-182-album-2023"
        );
    }

    #[test]
    fn test_derive_project_name_all_special_chars() {
        // Edge case: only special characters results in hyphen separator only
        assert_eq!(derive_project_name("!!!", "???"), "-");
    }

    #[test]
    fn test_derive_project_name_empty_strings() {
        // Edge case: empty strings result in hyphen separator only
        assert_eq!(derive_project_name("", ""), "-");
    }
}
