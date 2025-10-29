use crate::error::{Error, Result};
use crate::types::*;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Raw TOML configuration structure
/// This matches the album.toml file structure exactly
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // MVP: Some fields not yet used in Album struct
struct RawConfig {
    album: RawAlbumMetadata,
    artist: RawArtist,
    site: SiteConfig,
    #[serde(default)]
    track: Vec<RawTrack>,
    distribution: Distribution,
    hosting: RawHostingConfig,
    #[serde(default)]
    limits: Option<Limits>,
    rss: RssConfig,
}

#[derive(Debug, Deserialize)]
struct RawAlbumMetadata {
    title: String,
    artist: String,
    release_date: String, // Parse as NaiveDate
    summary: String,
    genre: Vec<String>,
    catalog_number: Option<String>,
    license: String,
    liner_notes: Option<String>, // Convert to PathBuf
}

#[derive(Debug, Deserialize)]
struct RawArtist {
    name: String,
    url: Option<String>,
    bio: Option<String>,
    rss_author_email: String,
}

#[derive(Debug, Deserialize)]
struct RawTrack {
    file: String, // Convert to PathBuf
    title: String,
    duration: Option<String>,    // Parse as Duration (format: "MM:SS")
    liner_notes: Option<String>, // Convert to PathBuf
}

#[derive(Debug, Deserialize)]
struct RawHostingConfig {
    cloudflare: CloudflareConfig,
}

/// Parse album.toml from a file path
pub fn parse_album_toml<P: AsRef<Path>>(path: P) -> Result<Album> {
    let content = fs::read_to_string(path)?;
    parse_album_toml_str(&content)
}

/// Parse album.toml from a string (useful for testing)
pub fn parse_album_toml_str(content: &str) -> Result<Album> {
    let raw: RawConfig = toml::from_str(content)?;

    // Parse release date
    let release_date = chrono::NaiveDate::parse_from_str(&raw.album.release_date, "%Y-%m-%d")
        .map_err(|e| Error::ConfigParse(format!("Invalid release_date: {}", e)))?;

    // Convert album metadata, validating paths
    let liner_notes = if let Some(notes_path) = raw.album.liner_notes {
        Some(validate_path(&notes_path, "album.liner_notes")?)
    } else {
        None
    };

    let metadata = AlbumMetadata {
        title: raw.album.title,
        artist: raw.album.artist,
        release_date,
        summary: raw.album.summary,
        genre: raw.album.genre,
        catalog_number: raw.album.catalog_number,
        license: raw.album.license,
        liner_notes,
    };

    // Convert artist
    let artist = Artist {
        name: raw.artist.name,
        url: raw.artist.url,
        bio: raw.artist.bio,
        rss_author_email: raw.artist.rss_author_email,
    };

    // Convert tracks, validating all paths
    let tracks: Result<Vec<Track>> = raw
        .track
        .into_iter()
        .map(|t| {
            let duration = if let Some(duration_str) = t.duration {
                Some(parse_duration(&duration_str)?)
            } else {
                None
            };

            let file = validate_path(&t.file, "track.file")?;
            let liner_notes = if let Some(notes_path) = t.liner_notes {
                Some(validate_path(&notes_path, "track.liner_notes")?)
            } else {
                None
            };

            Ok(Track {
                file,
                title: t.title,
                duration,
                liner_notes,
            })
        })
        .collect();

    Ok(Album {
        metadata,
        artist,
        site: raw.site,
        tracks: tracks?,
        distribution: raw.distribution,
        hosting: HostingConfig {
            cloudflare: raw.hosting.cloudflare,
        },
        rss: raw.rss,
    })
}

/// Validate and convert a path string to PathBuf.
///
/// This function prevents path traversal vulnerabilities by rejecting:
/// - Absolute paths (starting with `/` or Windows drive letters)
/// - Paths containing parent directory references (`..`)
///
/// # Security
///
/// This is critical for preventing malicious album.toml files from
/// accessing files outside the project directory.
///
/// # Arguments
///
/// * `path_str` - The path string from user input (album.toml)
/// * `field_name` - Name of the field for error messages
///
/// # Returns
///
/// A validated relative PathBuf, or an error if the path is unsafe
///
/// # Examples
///
/// ```text
/// // Valid relative paths
/// validate_path("audio/track.flac", "file")  → Ok(PathBuf)
/// validate_path("notes/album.md", "liner_notes")  → Ok(PathBuf)
///
/// // Invalid paths
/// validate_path("/etc/passwd", "file")  → Err("Absolute paths not allowed...")
/// validate_path("../../../etc/passwd", "file")  → Err("Parent directory references...")
/// validate_path("C:\\Windows\\System32", "file")  → Err("Absolute paths not allowed...")
/// ```
fn validate_path(path_str: &str, field_name: &str) -> Result<PathBuf> {
    let path = Path::new(path_str);

    // Reject absolute paths
    if path.is_absolute() {
        return Err(Error::ConfigParse(format!(
            "Absolute paths not allowed in '{}': '{}'. Use relative paths only.",
            field_name, path_str
        )));
    }

    // Check for parent directory references
    for component in path.components() {
        if component == std::path::Component::ParentDir {
            return Err(Error::ConfigParse(format!(
                "Parent directory references (..) not allowed in '{}': '{}'",
                field_name, path_str
            )));
        }
    }

    // Ensure path is not empty
    if path_str.trim().is_empty() {
        return Err(Error::ConfigParse(format!(
            "Empty path in '{}' field",
            field_name
        )));
    }

    Ok(path.to_path_buf())
}

/// Parse duration string in format "MM:SS" or "M:SS"
fn parse_duration(s: &str) -> Result<std::time::Duration> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(Error::ConfigParse(format!(
            "Invalid duration format '{}', expected MM:SS",
            s
        )));
    }

    let minutes: u64 = parts[0]
        .parse()
        .map_err(|_| Error::ConfigParse(format!("Invalid minutes in duration '{}'", s)))?;

    let seconds: u64 = parts[1]
        .parse()
        .map_err(|_| Error::ConfigParse(format!("Invalid seconds in duration '{}'", s)))?;

    if seconds >= 60 {
        return Err(Error::ConfigParse(format!(
            "Seconds must be < 60 in duration '{}'",
            s
        )));
    }

    Ok(std::time::Duration::from_secs(minutes * 60 + seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("5:23").unwrap().as_secs(), 323);
        assert_eq!(parse_duration("0:45").unwrap().as_secs(), 45);
        assert_eq!(parse_duration("12:00").unwrap().as_secs(), 720);
        assert!(parse_duration("5:60").is_err());
        assert!(parse_duration("invalid").is_err());
    }

    #[test]
    fn test_validate_path_valid_relative() {
        // Valid relative paths
        assert!(validate_path("audio/track.flac", "file").is_ok());
        assert!(validate_path("notes/album.md", "liner_notes").is_ok());
        assert!(validate_path("artwork/cover.jpg", "cover").is_ok());
        assert!(validate_path("subdir/nested/file.txt", "file").is_ok());
    }

    #[test]
    fn test_validate_path_rejects_absolute_unix() {
        // Unix absolute paths
        let result = validate_path("/etc/passwd", "file");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Absolute paths not allowed")
        );

        let result = validate_path("/root/.ssh/id_rsa", "file");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_path_rejects_absolute_windows() {
        // Windows absolute paths (only on Windows platform)
        #[cfg(target_os = "windows")]
        {
            let result = validate_path("C:\\Windows\\System32", "file");
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("Absolute paths not allowed")
            );
        }
    }

    #[test]
    fn test_validate_path_rejects_parent_dir() {
        // Parent directory references
        let result = validate_path("../etc/passwd", "file");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Parent directory references")
        );

        let result = validate_path("../../secret.txt", "file");
        assert!(result.is_err());

        let result = validate_path("audio/../../../etc/passwd", "file");
        assert!(result.is_err());

        // Multiple levels
        let result = validate_path("foo/bar/../../../baz", "file");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_path_rejects_empty() {
        let result = validate_path("", "file");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Empty path"));

        let result = validate_path("   ", "file");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_path_field_name_in_error() {
        let result = validate_path("/etc/passwd", "track.file");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("track.file"));

        let result = validate_path("../secret", "album.liner_notes");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("album.liner_notes")
        );
    }

    #[test]
    fn test_parse_minimal_config() {
        let toml = r##"
[album]
title = "Test Album"
artist = "Test Artist"
release_date = "2025-11-15"
summary = "A test album"
genre = ["experimental"]
license = "CC BY-NC-SA 4.0"

[artist]
name = "Test Artist"
rss_author_email = "test@example.com"

[site]
domain = "test.example.com"
theme = "default"
accent_color = "#ff6b35"

[[track]]
file = "audio/01-test.flac"
title = "Test Track"

[distribution]
streaming_enabled = true
download_enabled = false
pay_what_you_want = false
tip_jar_enabled = false
download_formats = ["flac"]

[hosting.cloudflare]
account_id = "test-account"
r2_bucket = "test-bucket"
pages_project = "test-project"

[rss]
enabled = true
        "##;

        let album = parse_album_toml_str(toml).unwrap();
        assert_eq!(album.metadata.title, "Test Album");
        assert_eq!(album.tracks.len(), 1);
        assert_eq!(album.tracks[0].title, "Test Track");
    }

    #[test]
    fn test_parse_config_rejects_path_traversal_in_track() {
        let toml = r##"
[album]
title = "Malicious Album"
artist = "Hacker"
release_date = "2025-11-15"
summary = "Test"
genre = ["experimental"]
license = "CC BY-NC-SA 4.0"

[artist]
name = "Test Artist"
rss_author_email = "test@example.com"

[site]
domain = "test.example.com"
theme = "default"
accent_color = "#ff6b35"

[[track]]
file = "../../../etc/passwd"
title = "Evil Track"

[distribution]
streaming_enabled = true
download_enabled = false
pay_what_you_want = false
tip_jar_enabled = false
download_formats = ["flac"]

[hosting.cloudflare]
account_id = "test-account"
r2_bucket = "test-bucket"
pages_project = "test-project"

[rss]
enabled = true
        "##;

        let result = parse_album_toml_str(toml);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Parent directory references")
        );
    }

    #[test]
    fn test_parse_config_rejects_absolute_path_in_track() {
        let toml = r##"
[album]
title = "Malicious Album"
artist = "Hacker"
release_date = "2025-11-15"
summary = "Test"
genre = ["experimental"]
license = "CC BY-NC-SA 4.0"

[artist]
name = "Test Artist"
rss_author_email = "test@example.com"

[site]
domain = "test.example.com"
theme = "default"
accent_color = "#ff6b35"

[[track]]
file = "/etc/passwd"
title = "Evil Track"

[distribution]
streaming_enabled = true
download_enabled = false
pay_what_you_want = false
tip_jar_enabled = false
download_formats = ["flac"]

[hosting.cloudflare]
account_id = "test-account"
r2_bucket = "test-bucket"
pages_project = "test-project"

[rss]
enabled = true
        "##;

        let result = parse_album_toml_str(toml);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Absolute paths not allowed")
        );
    }

    #[test]
    fn test_parse_config_rejects_path_traversal_in_liner_notes() {
        let toml = r##"
[album]
title = "Malicious Album"
artist = "Hacker"
release_date = "2025-11-15"
summary = "Test"
genre = ["experimental"]
license = "CC BY-NC-SA 4.0"
liner_notes = "../../etc/shadow"

[artist]
name = "Test Artist"
rss_author_email = "test@example.com"

[site]
domain = "test.example.com"
theme = "default"
accent_color = "#ff6b35"

[distribution]
streaming_enabled = true
download_enabled = false
pay_what_you_want = false
tip_jar_enabled = false
download_formats = ["flac"]

[hosting.cloudflare]
account_id = "test-account"
r2_bucket = "test-bucket"
pages_project = "test-project"

[rss]
enabled = true
        "##;

        let result = parse_album_toml_str(toml);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Parent directory references")
        );
    }
}
