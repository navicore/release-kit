use anyhow::{Context, Result};
use chrono::Local;
use lofty::prelude::*;
use lofty::probe::Probe;
use std::fs;
use std::path::{Path, PathBuf};
use toml;
use walkdir::WalkDir;

const AUDIO_EXTENSIONS: &[&str] = &["flac", "wav", "mp3", "ogg"];
const COVER_ART_NAMES: &[&str] = &[
    "cover.jpg",
    "cover.png",
    "artwork.jpg",
    "artwork.png",
    "folder.jpg",
    "folder.png",
    "album.jpg",
    "album.png",
];
const MAX_SCAN_DEPTH: usize = 2; // Maximum directory depth for audio file scanning

/// Escape a string for safe inclusion in TOML per TOML v1.0.0 spec
///
/// Handles the required escape sequences for TOML basic strings:
/// - Backslash (\\) -> \\\\
/// - Quote (\") -> \\\"
/// - Backspace (\b) -> \\b
/// - Form feed (\f) -> \\f
/// - Newline (\n) -> \\n
/// - Carriage return (\r) -> \\r
/// - Tab (\t) -> \\t
///
/// This manual implementation is used instead of toml crate serialization
/// because we're generating a template with comments and specific formatting,
/// not a complete TOML document. The toml crate's serialization doesn't
/// preserve comments or custom formatting.
///
/// See: https://toml.io/en/v1.0.0#string
fn toml_escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\x08', "\\b")
        .replace('\x0C', "\\f")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Validate email format
/// Checks for basic RFC 5322 compliance without full regex
fn is_valid_email(email: &str) -> bool {
    // Must have exactly one @ symbol
    let at_count = email.matches('@').count();
    if at_count != 1 {
        return false;
    }

    let parts: Vec<&str> = email.split('@').collect();
    let local = parts[0];
    let domain = parts[1];

    // Local part (before @) checks
    if local.is_empty() || local.len() > 64 {
        return false;
    }

    // Domain checks
    if domain.is_empty() || domain.len() > 255 {
        return false;
    }

    // Domain must have at least one dot
    if !domain.contains('.') {
        return false;
    }

    // Domain can't start/end with dot or hyphen
    if domain.starts_with('.')
        || domain.ends_with('.')
        || domain.starts_with('-')
        || domain.ends_with('-')
    {
        return false;
    }

    // No consecutive dots
    if domain.contains("..") {
        return false;
    }

    // Domain must have valid TLD (at least 2 chars after last dot)
    if let Some(last_dot) = domain.rfind('.') {
        let tld = &domain[last_dot + 1..];
        if tld.len() < 2 {
            return false;
        }
    }

    true
}

#[derive(Debug)]
struct DetectedTrack {
    path: PathBuf,
    title: String,
    duration: Option<String>,
    #[allow(dead_code)] // Will be used in future for format-specific handling
    format: String,
}

/// Initialize a new album project directory with smart defaults.
///
/// This command analyzes the given directory for audio files and cover art, then:
/// - Scans for audio files (FLAC, WAV, MP3, OGG)
/// - Extracts metadata (duration, format) from audio files
/// - Auto-generates track titles from filenames
/// - Detects cover art using common naming conventions
/// - Creates organized directory structure (audio/, artwork/, notes/)
/// - Generates album.toml with smart defaults
/// - Creates template liner notes in markdown
///
/// # Arguments
///
/// * `path` - Path to the directory to initialize (must exist)
///
/// # Errors
///
/// Returns an error if:
/// - The directory doesn't exist
/// - album.toml already exists in the directory
/// - File operations fail (permissions, disk space, etc.)
///
/// # Example
///
/// ```no_run
/// # use std::path::PathBuf;
/// # async fn example() -> anyhow::Result<()> {
/// release_kit::commands::init::run(PathBuf::from("my-album")).await?;
/// # Ok(())
/// # }
/// ```
pub async fn run(
    path: PathBuf,
    artist: Option<String>,
    album: Option<String>,
    email: Option<String>,
) -> Result<()> {
    println!("Initializing album directory: {}", path.display());

    if !path.exists() {
        anyhow::bail!(
            "Directory '{}' does not exist. Create it first: mkdir {}",
            path.display(),
            path.display()
        );
    }

    let album_toml_path = path.join("album.toml");
    if album_toml_path.exists() {
        anyhow::bail!(
            "album.toml already exists at {}\nHint: Delete it first or use a different directory",
            album_toml_path.display()
        );
    }

    println!("\nAnalyzing directory...");

    // Scan for audio files
    let audio_files = scan_audio_files(&path)?;

    if audio_files.is_empty() {
        println!("⚠ No audio files found");
        println!("Creating empty structure");
        create_empty_structure(&path)?;
        return Ok(());
    }

    println!("✓ Found {} audio file(s)", audio_files.len());

    // Detect cover art
    let cover_art = detect_cover_art(&path)?;
    if let Some(ref cover) = cover_art {
        println!("✓ Detected cover art: {}", cover.display());
    }

    // Extract metadata from audio files
    let tracks = extract_track_metadata(&audio_files)?;
    println!("✓ Extracted metadata from {} track(s)", tracks.len());

    // Create directory structure
    create_directory_structure(&path)?;

    // Move/copy files to proper locations
    organize_files(&path, &audio_files, &cover_art)?;

    // Generate album.toml
    generate_album_toml(
        &path,
        &tracks,
        artist.as_deref(),
        album.as_deref(),
        email.as_deref(),
    )?;

    // Generate template notes
    generate_notes_template(&path)?;

    println!("\n✓ Initialization complete!");
    println!("\nGenerated structure:");
    println!("  {}/", path.display());
    println!("  ├── album.toml           ← Edit this to set artist name, etc.");
    println!("  ├── artwork/");
    if cover_art.is_some() {
        println!("  │   └── cover.jpg");
    }
    println!("  ├── audio/");
    for track in &tracks {
        println!(
            "  │   └── {}",
            track.path.file_name().unwrap().to_string_lossy()
        );
    }
    println!("  └── notes/");
    println!("      └── album.md         ← Add liner notes here");

    println!("\nNext steps:");
    println!("  1. Edit album.toml (set artist name, release date, summary)");
    println!("  2. Add liner notes to notes/album.md");
    println!("  3. Preview: release-kit preview {}", path.display());

    Ok(())
}

/// Scan directory for supported audio files.
///
/// Recursively searches up to `MAX_SCAN_DEPTH` levels for files with
/// supported audio extensions (FLAC, WAV, MP3, OGG).
///
/// # Arguments
///
/// * `dir` - Directory to scan
///
/// # Returns
///
/// Sorted vector of paths to audio files found
fn scan_audio_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut audio_files = Vec::new();

    for entry in WalkDir::new(dir)
        .max_depth(MAX_SCAN_DEPTH)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        if let Some(ext) = entry.path().extension()
            && AUDIO_EXTENSIONS.contains(&ext.to_string_lossy().to_lowercase().as_str())
        {
            audio_files.push(entry.path().to_path_buf());
        }
    }

    // Sort by filename for consistent ordering
    audio_files.sort();

    Ok(audio_files)
}

fn detect_cover_art(dir: &Path) -> Result<Option<PathBuf>> {
    // Try specific cover art names first
    for name in COVER_ART_NAMES {
        let path = dir.join(name);
        if path.exists() {
            return Ok(Some(path));
        }
    }

    // Fallback: find first JPG or PNG
    for entry in WalkDir::new(dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        if let Some(ext) = entry.path().extension() {
            let ext_lower = ext.to_string_lossy().to_lowercase();
            if ext_lower == "jpg" || ext_lower == "jpeg" || ext_lower == "png" {
                return Ok(Some(entry.path().to_path_buf()));
            }
        }
    }

    Ok(None)
}

fn extract_track_metadata(audio_files: &[PathBuf]) -> Result<Vec<DetectedTrack>> {
    let mut tracks = Vec::new();

    for (idx, path) in audio_files.iter().enumerate() {
        let title = extract_track_title(path, idx + 1);

        let (duration, format) = match Probe::open(path)
            .context("Failed to open audio file")?
            .read()
        {
            Ok(tagged_file) => {
                let properties = tagged_file.properties();
                let duration_secs = properties.duration().as_secs();
                let duration_str = format!("{}:{:02}", duration_secs / 60, duration_secs % 60);

                // Get format from file extension
                let format = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_uppercase())
                    .unwrap_or_else(|| "Audio".to_string());

                (Some(duration_str), format)
            }
            Err(_) => (None, "Audio".to_string()),
        };

        tracks.push(DetectedTrack {
            path: path.clone(),
            title,
            duration,
            format,
        });
    }

    Ok(tracks)
}

/// Extract a human-readable track title from a filename.
///
/// Strips common track number prefixes (01-, 01_, track-01, etc.),
/// replaces underscores/hyphens with spaces, and title-cases words.
///
/// # Arguments
///
/// * `path` - Path to audio file
/// * `track_number` - Track number (used as fallback if title extraction fails)
///
/// # Returns
///
/// Title-cased track name, or "Track N" if extraction fails
///
/// # Examples
///
/// - `01-infrastructure-hum.flac` → "Infrastructure Hum"
/// - `02_resonant_decay.flac` → "Resonant Decay"
/// - `track-01.flac` → "Track 1"
fn extract_track_title(path: &Path, track_number: usize) -> String {
    let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Track");

    // Remove common track number prefixes
    let cleaned = filename
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '-' || c == '_' || c == '.')
        .trim_start_matches("track")
        .trim_start_matches('-')
        .trim_start_matches('_')
        .trim();

    // If empty or only contains digits, use fallback
    if cleaned.is_empty() || cleaned.chars().all(|c| c.is_ascii_digit()) {
        return format!("Track {}", track_number);
    }

    // Replace underscores/hyphens with spaces and title-case
    let normalized = cleaned.replace(['_', '-'], " ");
    let words: Vec<String> = normalized
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect();

    words.join(" ")
}

fn create_directory_structure(base: &Path) -> Result<()> {
    fs::create_dir_all(base.join("artwork"))?;
    fs::create_dir_all(base.join("audio"))?;
    fs::create_dir_all(base.join("notes"))?;
    Ok(())
}

fn create_empty_structure(base: &Path) -> Result<()> {
    create_directory_structure(base)?;
    generate_album_toml(base, &[], None, None, None)?;
    generate_notes_template(base)?;

    println!("\n✓ Created empty structure");
    println!("\nNext steps:");
    println!("  1. Add audio files to audio/");
    println!("  2. Add cover art to artwork/");
    println!("  3. Edit album.toml");

    Ok(())
}

fn organize_files(base: &Path, audio_files: &[PathBuf], cover_art: &Option<PathBuf>) -> Result<()> {
    // Move/copy audio files to audio/
    for audio_file in audio_files {
        let filename = audio_file.file_name().unwrap();
        let dest = base.join("audio").join(filename);

        // If file is already in the target location, skip
        // Only compare if destination exists to avoid canonicalization errors
        if dest.exists()
            && let (Ok(src_canon), Ok(dst_canon)) = (audio_file.canonicalize(), dest.canonicalize())
            && src_canon == dst_canon
        {
            continue;
        }

        fs::copy(audio_file, &dest).context("Failed to copy audio file")?;
    }

    // Move/copy cover art to artwork/
    if let Some(cover_path) = cover_art {
        let ext = cover_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg");
        let dest = base.join("artwork").join(format!("cover.{}", ext));

        // If file is already in the target location, skip
        // Only compare if destination exists to avoid canonicalization errors
        let should_copy = if dest.exists() {
            if let (Ok(src_canon), Ok(dst_canon)) = (cover_path.canonicalize(), dest.canonicalize())
            {
                src_canon != dst_canon
            } else {
                true // Copy if canonicalization fails
            }
        } else {
            true // Copy if destination doesn't exist
        };

        if should_copy {
            fs::copy(cover_path, &dest).context("Failed to copy cover art")?;
        }
    }

    Ok(())
}

fn generate_album_toml(
    base: &Path,
    tracks: &[DetectedTrack],
    artist: Option<&str>,
    album: Option<&str>,
    email: Option<&str>,
) -> Result<()> {
    let today = Local::now().format("%Y-%m-%d").to_string();

    // Validate email if provided
    // Note: We use nested if instead of if-let chains for broader Rust version compatibility
    #[allow(clippy::collapsible_if)]
    if let Some(e) = email {
        if !is_valid_email(e) {
            anyhow::bail!("Invalid email format: '{}'", e);
        }
    }

    // Escape user input for safe TOML inclusion using toml crate
    let artist_name = toml_escape_string(artist.unwrap_or("Artist Name"));
    let album_title = toml_escape_string(album.unwrap_or("My Album"));
    let artist_email = toml_escape_string(email.unwrap_or("artist@example.com"));

    let artist_comment = if artist.is_some() {
        ""
    } else {
        "  # TODO: Set artist name"
    };
    let album_comment = if album.is_some() {
        ""
    } else {
        "  # TODO: Set album title"
    };
    let email_comment = if email.is_some() {
        ""
    } else {
        "  # TODO: Set email"
    };

    let mut toml = format!(
        "# Generated by release-kit init\n\
# Edit this file to customize your album\n\
\n\
[album]\n\
title = \"{album_title}\"{album_comment}\n\
artist = \"{artist_name}\"{artist_comment}\n\
release_date = \"{today}\"  # TODO: Set release date\n\
summary = \"Description of this album\"  # TODO: Add summary\n\
genre = [\"experimental\"]  # TODO: Set genres\n\
license = \"CC BY-NC-SA 4.0\"\n\
liner_notes = \"notes/album.md\"\n\
\n\
[artist]\n\
name = \"{artist_name}\"{artist_comment}\n\
rss_author_email = \"{artist_email}\"{email_comment}\n\
\n\
[site]\n\
domain = \"my-album.example.com\"  # TODO: Set domain\n\
theme = \"default\"\n\
accent_color = \"#ff6b35\"\n\
\n\
"
    );

    if tracks.is_empty() {
        toml.push_str(
            r##"# Add tracks here as you add audio files
# [[track]]
# file = "audio/01-track-name.flac"
# title = "Track Name"
# duration = "5:23"
# liner_notes = "notes/track-01.md"  # Optional

"##,
        );
    } else {
        toml.push_str("# Auto-detected tracks (edit titles/add liner notes as needed)\n");
        for track in tracks {
            let filename = track
                .path
                .file_name()
                .context("Track path has no filename")?
                .to_string_lossy();
            let filename = toml_escape_string(&filename);
            let title = toml_escape_string(&track.title);
            toml.push_str("[[track]]\n");
            toml.push_str(&format!("file = \"audio/{}\"\n", filename));
            toml.push_str(&format!("title = \"{}\"\n", title));
            if let Some(ref duration) = track.duration {
                toml.push_str(&format!("duration = \"{}\"  # Auto-detected\n", duration));
            }
            toml.push_str("# liner_notes = \"notes/track-XX.md\"  # Optional\n");
            toml.push('\n');
        }
    }

    toml.push_str(
        r##"[distribution]
streaming_enabled = true
download_enabled = false
pay_what_you_want = false
tip_jar_enabled = false
download_formats = ["flac", "mp3-320"]

[hosting.cloudflare]
# Optional: Custom subdomain for your domain (e.g., "my-album" -> my-album.yourdomain.com)
# Leave empty to use the default .pages.dev domain
# subdomain = "my-album"

[rss]
enabled = true
"##,
    );

    // Validate the generated TOML can be parsed
    toml::from_str::<toml::Value>(&toml)
        .context("Generated TOML is invalid - this is a bug in the template generator")?;

    fs::write(base.join("album.toml"), toml)?;

    Ok(())
}

fn generate_notes_template(base: &Path) -> Result<()> {
    let template = r##"# Album Notes

Write about your album here. This is markdown, so you can use:

- **Bold** and *italic* text
- Lists and bullet points
- Links and images
- Code blocks
- Anything else markdown supports

## Recording Process

Describe how you made this album...

## Equipment

List the gear you used...

## Credits

Thanks to...
"##;

    fs::write(base.join("notes").join("album.md"), template)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper to create a test directory with optional audio files
    fn create_test_dir_with_audio(files: &[&str]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for file in files {
            let path = dir.path().join(file);
            fs::write(&path, b"fake audio data").unwrap();
        }
        dir
    }

    /// Helper to create a test directory with cover art
    fn create_test_dir_with_cover(filename: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(filename);
        fs::write(&path, b"fake image data").unwrap();
        dir
    }

    #[test]
    fn test_extract_track_title_basic() {
        // Test basic filename with dash separator
        let path = Path::new("01-infrastructure-hum.flac");
        assert_eq!(extract_track_title(path, 1), "Infrastructure Hum");

        // Test with underscore separator
        let path = Path::new("02_resonant_decay.flac");
        assert_eq!(extract_track_title(path, 2), "Resonant Decay");

        // Test mixed separators
        let path = Path::new("03-some_track-name.wav");
        assert_eq!(extract_track_title(path, 3), "Some Track Name");
    }

    #[test]
    fn test_extract_track_title_edge_cases() {
        // Test filename with only track number
        let path = Path::new("01.flac");
        assert_eq!(extract_track_title(path, 1), "Track 1");

        // Test filename with "track" prefix - now correctly returns "Track 1"
        let path = Path::new("track-01.flac");
        assert_eq!(extract_track_title(path, 1), "Track 1");

        // Test filename with no number prefix
        let path = Path::new("ambient-soundscape.mp3");
        assert_eq!(extract_track_title(path, 5), "Ambient Soundscape");

        // Test with dots in prefix - strips leading patterns leaving only name
        let path = Path::new("01.02-track-name.ogg");
        assert_eq!(extract_track_title(path, 1), "Name");
    }

    #[test]
    fn test_extract_track_title_case_handling() {
        // Test all lowercase
        let path = Path::new("01-lowercase-track.flac");
        assert_eq!(extract_track_title(path, 1), "Lowercase Track");

        // Test all uppercase
        let path = Path::new("02-UPPERCASE-TRACK.flac");
        assert_eq!(extract_track_title(path, 2), "Uppercase Track");

        // Test mixed case
        let path = Path::new("03-MiXeD-CaSe.flac");
        assert_eq!(extract_track_title(path, 3), "Mixed Case");
    }

    #[test]
    fn test_scan_audio_files_empty_directory() {
        let dir = TempDir::new().unwrap();
        let result = scan_audio_files(dir.path()).unwrap();
        assert!(
            result.is_empty(),
            "Empty directory should return no audio files"
        );
    }

    #[test]
    fn test_scan_audio_files_finds_supported_formats() {
        let dir =
            create_test_dir_with_audio(&["track1.flac", "track2.wav", "track3.mp3", "track4.ogg"]);

        let result = scan_audio_files(dir.path()).unwrap();
        assert_eq!(result.len(), 4, "Should find all 4 audio files");

        // Check that files are sorted alphabetically
        let filenames: Vec<_> = result
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        // Files are sorted: track1.flac < track2.wav < track3.mp3 < track4.ogg
        assert_eq!(
            filenames,
            vec!["track1.flac", "track2.wav", "track3.mp3", "track4.ogg"]
        );
    }

    #[test]
    fn test_scan_audio_files_ignores_non_audio() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("track.flac"), b"audio").unwrap();
        fs::write(dir.path().join("readme.txt"), b"text").unwrap();
        fs::write(dir.path().join("cover.jpg"), b"image").unwrap();
        fs::write(dir.path().join("data.json"), b"json").unwrap();

        let result = scan_audio_files(dir.path()).unwrap();
        assert_eq!(result.len(), 1, "Should only find audio files");
        assert!(result[0].ends_with("track.flac"));
    }

    #[test]
    fn test_scan_audio_files_case_insensitive_extensions() {
        let dir =
            create_test_dir_with_audio(&["track1.FLAC", "track2.Wav", "track3.MP3", "track4.OGG"]);

        let result = scan_audio_files(dir.path()).unwrap();
        assert_eq!(
            result.len(),
            4,
            "Should find files with uppercase extensions"
        );
    }

    #[test]
    fn test_scan_audio_files_respects_max_depth() {
        let dir = TempDir::new().unwrap();

        // Create nested directories beyond MAX_SCAN_DEPTH
        fs::write(dir.path().join("track1.flac"), b"audio").unwrap();

        let subdir1 = dir.path().join("subdir1");
        fs::create_dir(&subdir1).unwrap();
        fs::write(subdir1.join("track2.flac"), b"audio").unwrap();

        let subdir2 = subdir1.join("subdir2");
        fs::create_dir(&subdir2).unwrap();
        fs::write(subdir2.join("track3.flac"), b"audio").unwrap();

        let subdir3 = subdir2.join("subdir3");
        fs::create_dir(&subdir3).unwrap();
        fs::write(subdir3.join("track4.flac"), b"audio").unwrap();

        let result = scan_audio_files(dir.path()).unwrap();

        // MAX_SCAN_DEPTH is 2, which means we can traverse 2 levels deep
        // The test verifies we don't find files at depth 3 or beyond
        assert!(
            result.len() <= 3,
            "Should respect MAX_SCAN_DEPTH and not find all 4 files"
        );
        assert!(
            !result.is_empty(),
            "Should find at least the root level file"
        );

        // Verify we don't find the deeply nested file
        let has_track4 = result.iter().any(|p| p.ends_with("track4.flac"));
        assert!(!has_track4, "Should not find track4.flac at depth 3");
    }

    #[test]
    fn test_detect_cover_art_standard_names() {
        // Test each standard cover art name
        for name in COVER_ART_NAMES {
            let dir = create_test_dir_with_cover(name);
            let result = detect_cover_art(dir.path()).unwrap();
            assert!(result.is_some(), "Should detect cover art named '{}'", name);
            assert!(result.unwrap().ends_with(name));
        }
    }

    #[test]
    fn test_detect_cover_art_priority() {
        // Create directory with multiple potential cover art files
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("cover.jpg"), b"image1").unwrap();
        fs::write(dir.path().join("folder.png"), b"image2").unwrap();
        fs::write(dir.path().join("random.jpg"), b"image3").unwrap();

        let result = detect_cover_art(dir.path()).unwrap();
        assert!(result.is_some());
        // Should prefer "cover.jpg" (first in COVER_ART_NAMES list)
        assert!(result.unwrap().ends_with("cover.jpg"));
    }

    #[test]
    fn test_detect_cover_art_fallback() {
        // Create directory with no standard names, just a random JPG
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("random-image.jpg"), b"image").unwrap();

        let result = detect_cover_art(dir.path()).unwrap();
        assert!(result.is_some(), "Should fall back to any JPG/PNG");
    }

    #[test]
    fn test_detect_cover_art_none() {
        // Directory with no image files
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("audio.flac"), b"audio").unwrap();
        fs::write(dir.path().join("readme.txt"), b"text").unwrap();

        let result = detect_cover_art(dir.path()).unwrap();
        assert!(
            result.is_none(),
            "Should return None when no cover art found"
        );
    }

    #[test]
    fn test_detect_cover_art_case_insensitive() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("COVER.JPG"), b"image").unwrap();

        let result = detect_cover_art(dir.path()).unwrap();
        assert!(result.is_some(), "Should detect uppercase extensions");
    }

    #[test]
    fn test_create_directory_structure() {
        let dir = TempDir::new().unwrap();
        create_directory_structure(dir.path()).unwrap();

        assert!(dir.path().join("artwork").is_dir());
        assert!(dir.path().join("audio").is_dir());
        assert!(dir.path().join("notes").is_dir());
    }

    #[test]
    fn test_create_directory_structure_idempotent() {
        let dir = TempDir::new().unwrap();

        // Create twice - should not error
        create_directory_structure(dir.path()).unwrap();
        create_directory_structure(dir.path()).unwrap();

        assert!(dir.path().join("artwork").is_dir());
        assert!(dir.path().join("audio").is_dir());
        assert!(dir.path().join("notes").is_dir());
    }

    #[test]
    fn test_generate_album_toml_empty_tracks() {
        let dir = TempDir::new().unwrap();
        generate_album_toml(dir.path(), &[], None, None, None).unwrap();

        let toml_path = dir.path().join("album.toml");
        assert!(toml_path.exists(), "album.toml should be created");

        let content = fs::read_to_string(&toml_path).unwrap();
        assert!(content.contains("[album]"));
        assert!(content.contains("title = \"My Album\""));
        assert!(content.contains("# Add tracks here"));
        // The template includes a commented-out [[track]] example, which is fine
    }

    #[test]
    fn test_generate_album_toml_with_tracks() {
        let dir = TempDir::new().unwrap();
        let tracks = vec![
            DetectedTrack {
                path: PathBuf::from("01-first-track.flac"),
                title: "First Track".to_string(),
                duration: Some("5:23".to_string()),
                format: "FLAC".to_string(),
            },
            DetectedTrack {
                path: PathBuf::from("02-second-track.flac"),
                title: "Second Track".to_string(),
                duration: Some("3:45".to_string()),
                format: "FLAC".to_string(),
            },
        ];

        generate_album_toml(dir.path(), &tracks, None, None, None).unwrap();

        let content = fs::read_to_string(dir.path().join("album.toml")).unwrap();
        assert!(content.contains("[[track]]"));
        assert!(content.contains("file = \"audio/01-first-track.flac\""));
        assert!(content.contains("title = \"First Track\""));
        assert!(content.contains("duration = \"5:23\""));
        assert!(content.contains("file = \"audio/02-second-track.flac\""));
        assert!(content.contains("title = \"Second Track\""));
        assert!(content.contains("duration = \"3:45\""));
    }

    #[test]
    fn test_generate_album_toml_includes_required_sections() {
        let dir = TempDir::new().unwrap();
        generate_album_toml(dir.path(), &[], None, None, None).unwrap();

        let content = fs::read_to_string(dir.path().join("album.toml")).unwrap();

        // Check all required sections
        assert!(content.contains("[album]"));
        assert!(content.contains("[artist]"));
        assert!(content.contains("[site]"));
        assert!(content.contains("[distribution]"));
        assert!(content.contains("[hosting.cloudflare]"));
        assert!(content.contains("[rss]"));

        // Check required fields
        assert!(content.contains("streaming_enabled"));
        assert!(content.contains("download_enabled"));
        assert!(content.contains("license = \"CC BY-NC-SA 4.0\""));
    }

    #[test]
    fn test_generate_notes_template() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("notes")).unwrap();

        generate_notes_template(dir.path()).unwrap();

        let notes_path = dir.path().join("notes").join("album.md");
        assert!(notes_path.exists(), "album.md should be created");

        let content = fs::read_to_string(&notes_path).unwrap();
        assert!(content.contains("# Album Notes"));
        assert!(content.contains("## Recording Process"));
        assert!(content.contains("## Equipment"));
        assert!(content.contains("## Credits"));
    }

    #[test]
    fn test_organize_files_copies_audio() {
        let dir = TempDir::new().unwrap();
        let audio_file = dir.path().join("track.flac");
        fs::write(&audio_file, b"audio data").unwrap();

        create_directory_structure(dir.path()).unwrap();
        organize_files(dir.path(), std::slice::from_ref(&audio_file), &None).unwrap();

        let dest = dir.path().join("audio").join("track.flac");
        assert!(dest.exists(), "Audio file should be copied to audio/");

        let content = fs::read_to_string(&dest).unwrap();
        assert_eq!(content, "audio data");
    }

    #[test]
    fn test_organize_files_copies_cover_art() {
        let dir = TempDir::new().unwrap();
        let cover_file = dir.path().join("my-cover.jpg");
        fs::write(&cover_file, b"image data").unwrap();

        create_directory_structure(dir.path()).unwrap();
        organize_files(dir.path(), &[], &Some(cover_file)).unwrap();

        let dest = dir.path().join("artwork").join("cover.jpg");
        assert!(
            dest.exists(),
            "Cover art should be copied to artwork/cover.jpg"
        );

        let content = fs::read_to_string(&dest).unwrap();
        assert_eq!(content, "image data");
    }

    #[test]
    fn test_organize_files_skips_already_organized() {
        let dir = TempDir::new().unwrap();
        create_directory_structure(dir.path()).unwrap();

        // Create file already in target location
        let audio_dir = dir.path().join("audio");
        let audio_file = audio_dir.join("track.flac");
        fs::write(&audio_file, b"original data").unwrap();

        // Try to organize - should skip
        organize_files(dir.path(), std::slice::from_ref(&audio_file), &None).unwrap();

        // Content should remain unchanged
        let content = fs::read_to_string(&audio_file).unwrap();
        assert_eq!(
            content, "original data",
            "Should not overwrite existing file"
        );
    }

    #[test]
    fn test_organize_files_preserves_extension() {
        let dir = TempDir::new().unwrap();
        let png_cover = dir.path().join("cover.png");
        fs::write(&png_cover, b"png image").unwrap();

        create_directory_structure(dir.path()).unwrap();
        organize_files(dir.path(), &[], &Some(png_cover)).unwrap();

        let dest = dir.path().join("artwork").join("cover.png");
        assert!(dest.exists(), "Should preserve .png extension");
    }

    #[test]
    fn test_create_empty_structure() {
        let dir = TempDir::new().unwrap();
        create_empty_structure(dir.path()).unwrap();

        // Check directories created
        assert!(dir.path().join("artwork").is_dir());
        assert!(dir.path().join("audio").is_dir());
        assert!(dir.path().join("notes").is_dir());

        // Check files created
        assert!(dir.path().join("album.toml").exists());
        assert!(dir.path().join("notes").join("album.md").exists());
    }

    #[test]
    fn test_toml_escape_string() {
        // Test quote escaping
        assert_eq!(toml_escape_string(r#"Test "Quote""#), r#"Test \"Quote\""#);

        // Test backslash escaping
        assert_eq!(toml_escape_string(r"Test\Back"), r"Test\\Back");

        // Test newline escaping
        assert_eq!(toml_escape_string("Test\nNewline"), r"Test\nNewline");

        // Test combined
        assert_eq!(
            toml_escape_string(r#"Test "Quote" and\Back"#),
            r#"Test \"Quote\" and\\Back"#
        );

        // Test normal string (no escaping needed)
        assert_eq!(toml_escape_string("Normal String"), "Normal String");
    }

    #[test]
    fn test_is_valid_email() {
        // Valid emails
        assert!(is_valid_email("user@example.com"));
        assert!(is_valid_email("test.user@domain.co.uk"));
        assert!(is_valid_email("name+tag@example.org"));

        // Invalid emails - missing @
        assert!(!is_valid_email("user"));
        assert!(!is_valid_email(""));

        // Invalid emails - multiple @
        assert!(!is_valid_email("user@@example.com"));
        assert!(!is_valid_email("user@name@example.com"));

        // Invalid emails - missing parts
        assert!(!is_valid_email("@example.com"));
        assert!(!is_valid_email("user@"));

        // Invalid emails - invalid domain
        assert!(!is_valid_email("user@domain")); // No TLD
        assert!(!is_valid_email("user@.com")); // Domain starts with dot
        assert!(!is_valid_email("user@domain.")); // Domain ends with dot
        assert!(!is_valid_email("user@domain.c")); // TLD too short
        assert!(!is_valid_email("user@domain..com")); // Consecutive dots

        // Invalid emails - local part too long
        let long_local = "a".repeat(65);
        assert!(!is_valid_email(&format!("{}@example.com", long_local)));
    }

    #[test]
    fn test_generate_album_toml_with_artist_and_album() {
        let dir = TempDir::new().unwrap();
        generate_album_toml(
            dir.path(),
            &[],
            Some("Test Artist"),
            Some("Test Album"),
            None,
        )
        .unwrap();

        let content = fs::read_to_string(dir.path().join("album.toml")).unwrap();

        // Should have escaped values
        assert!(content.contains(r#"title = "Test Album""#));
        assert!(content.contains(r#"artist = "Test Artist""#));

        // Should NOT have TODO comments for provided values
        assert!(!content.contains("TODO: Set album title"));
        assert!(!content.contains("TODO: Set artist name"));

        // Should still have TODO for email
        assert!(content.contains("TODO: Set email"));
    }

    #[test]
    fn test_generate_album_toml_with_special_characters() {
        let dir = TempDir::new().unwrap();
        generate_album_toml(
            dir.path(),
            &[],
            Some(r#"Artist "The Quote""#),
            Some(r"Album\Backslash"),
            Some("test@example.com"),
        )
        .unwrap();

        let content = fs::read_to_string(dir.path().join("album.toml")).unwrap();

        // Should have escaped quotes
        assert!(content.contains(r#"Artist \"The Quote\""#));

        // Should have escaped backslash
        assert!(content.contains(r"Album\\Backslash"));

        // Should have valid email
        assert!(content.contains(r#"rss_author_email = "test@example.com""#));
    }

    #[test]
    fn test_generate_album_toml_invalid_email() {
        let dir = TempDir::new().unwrap();
        let result = generate_album_toml(
            dir.path(),
            &[],
            Some("Artist"),
            Some("Album"),
            Some("invalid-email"),
        );

        // Should fail with invalid email
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid email"));
    }

    #[test]
    fn test_toml_validation_with_special_chars() {
        // Ensure TOML validation catches escaping errors
        let dir = TempDir::new().unwrap();

        // This should succeed - special chars are properly escaped
        let result = generate_album_toml(
            dir.path(),
            &[],
            Some(r#"Artist "Name""#),
            Some(r"Album\Title"),
            Some("test@example.com"),
        );

        assert!(
            result.is_ok(),
            "Should successfully generate TOML with special characters"
        );

        // Verify the file can be parsed
        let toml_content = fs::read_to_string(dir.path().join("album.toml")).unwrap();
        let parsed = toml::from_str::<toml::Value>(&toml_content);
        assert!(parsed.is_ok(), "Generated TOML should be parseable");
    }

    #[test]
    fn test_generate_album_toml_with_tracks_validates() {
        // Test that generated TOML with tracks is valid
        let dir = TempDir::new().unwrap();
        let tracks = vec![DetectedTrack {
            path: PathBuf::from("audio/01-test.flac"),
            title: r#"Track "With" Quotes"#.to_string(),
            duration: Some("3:45".to_string()),
            format: "flac".to_string(),
        }];

        generate_album_toml(dir.path(), &tracks, Some("Artist"), Some("Album"), None).unwrap();

        // Verify TOML can be parsed
        let toml_content = fs::read_to_string(dir.path().join("album.toml")).unwrap();
        let parsed = toml::from_str::<toml::Value>(&toml_content);
        assert!(
            parsed.is_ok(),
            "Generated TOML with tracks should be parseable"
        );
    }
}
