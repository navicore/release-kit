use crate::error::{Error, Result};
use crate::types::*;
use serde::Deserialize;
use std::fs;
use std::path::Path;

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

    // Convert album metadata
    let metadata = AlbumMetadata {
        title: raw.album.title,
        artist: raw.album.artist,
        release_date,
        summary: raw.album.summary,
        genre: raw.album.genre,
        catalog_number: raw.album.catalog_number,
        license: raw.album.license,
        liner_notes: raw.album.liner_notes.map(|s| s.into()),
    };

    // Convert artist
    let artist = Artist {
        name: raw.artist.name,
        url: raw.artist.url,
        bio: raw.artist.bio,
        rss_author_email: raw.artist.rss_author_email,
    };

    // Convert tracks
    let tracks: Result<Vec<Track>> = raw
        .track
        .into_iter()
        .map(|t| {
            let duration = if let Some(duration_str) = t.duration {
                Some(parse_duration(&duration_str)?)
            } else {
                None
            };

            Ok(Track {
                file: t.file.into(),
                title: t.title,
                duration,
                liner_notes: t.liner_notes.map(|s| s.into()),
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
}
