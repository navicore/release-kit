use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Complete album configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub metadata: AlbumMetadata,
    pub artist: Artist,
    pub site: SiteConfig,
    pub tracks: Vec<Track>,
    pub distribution: Distribution,
    pub hosting: HostingConfig,
    pub rss: RssConfig,
}

/// Album metadata and description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumMetadata {
    pub title: String,
    pub artist: String,
    pub release_date: NaiveDate,
    pub summary: String,
    pub genre: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_number: Option<String>,
    pub license: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liner_notes: Option<PathBuf>,
}

/// Artist information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    pub rss_author_email: String,
}

/// Site configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteConfig {
    pub domain: String,
    pub theme: String,
    pub accent_color: String,
}

/// Individual track
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub file: PathBuf,
    pub title: String,
    /// Duration in seconds (auto-detected if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<Duration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liner_notes: Option<PathBuf>,
}

impl Track {
    /// Get the filename component for use in URLs
    pub fn file_name(&self) -> String {
        self.file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    }

    /// Get a URL-safe slug from the title
    pub fn slug(&self) -> String {
        self.title
            .to_lowercase()
            .replace(char::is_whitespace, "-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect()
    }
}

/// Distribution settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    pub streaming_enabled: bool,
    pub download_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_price: Option<f64>,
    pub pay_what_you_want: bool,
    pub tip_jar_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip_suggested_amounts: Option<Vec<u32>>,
    pub download_formats: Vec<String>,
}

/// Hosting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostingConfig {
    pub cloudflare: CloudflareConfig,
}

/// Cloudflare-specific hosting config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareConfig {
    pub account_id: String,
    pub r2_bucket: String,
    pub pages_project: String,
    /// Custom subdomain for album (e.g., "my-album" -> my-album.yourdomain.com)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subdomain: Option<String>,
}

/// Bandwidth limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limits {
    pub max_monthly_bandwidth_gb: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent_streams: Option<u32>,
}

/// RSS feed configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RssConfig {
    pub enabled: bool,
}

/// Artwork files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artwork {
    pub cover: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<PathBuf>,
}

/// Helper to format duration as MM:SS
pub fn format_duration(duration: Option<Duration>) -> String {
    match duration {
        Some(d) => {
            let total_secs = d.as_secs();
            let mins = total_secs / 60;
            let secs = total_secs % 60;
            format!("{}:{:02}", mins, secs)
        }
        None => "?:??".to_string(),
    }
}
