use anyhow::{Context, Result};
use lofty::prelude::*;
use lofty::probe::Probe;
use release_kit_core::config::parse_album_toml;
use std::path::{Path, PathBuf};

/// Validation result tracker
struct ValidationResults {
    errors: Vec<String>,
    warnings: Vec<String>,
}

impl ValidationResults {
    fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }

    fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Validate album directory and configuration for deployment readiness.
///
/// Checks:
/// - Directory structure exists
/// - album.toml is valid and parseable
/// - Required metadata fields are complete
/// - Audio files exist and are readable
/// - Cover art exists (warns if missing)
/// - Liner notes exist if referenced
/// - Audio file formats are supported
///
/// Returns Ok if validation passes, Err with detailed report if not.
pub async fn run(path: PathBuf) -> Result<()> {
    println!("🔍 Validating album at: {}\n", path.display());

    let mut results = ValidationResults::new();

    // Check directory exists
    if !path.exists() {
        anyhow::bail!("Album directory does not exist: {}", path.display());
    }

    // Check album.toml exists and is parseable
    let config_path = path.join("album.toml");
    if !config_path.exists() {
        anyhow::bail!(
            "album.toml not found in {}\nRun 'release-kit init {}' first",
            path.display(),
            path.display()
        );
    }

    let album = parse_album_toml(&config_path).context("Failed to parse album.toml")?;

    println!("✓ Configuration loaded");
    println!(
        "  Album: {} by {}",
        album.metadata.title, album.metadata.artist
    );
    println!("  Tracks: {}", album.tracks.len());
    println!();

    // Validate metadata completeness
    validate_metadata(&album, &mut results);

    // Validate directory structure
    validate_directories(&path, &mut results);

    // Validate audio files
    validate_audio_files(&path, &album, &mut results);

    // Validate cover art (warning only)
    validate_cover_art(&path, &mut results);

    // Validate liner notes
    validate_liner_notes(&path, &album, &mut results);

    // Print results
    print_results(&results);

    if !results.is_valid() {
        anyhow::bail!("Validation failed with {} error(s)", results.errors.len());
    }

    println!("\n✅ Validation passed! Album is ready for deployment.");
    Ok(())
}

fn validate_metadata(album: &release_kit_core::types::Album, results: &mut ValidationResults) {
    println!("📋 Validating metadata...");

    // Check for TODO placeholders
    if album.metadata.title.contains("TODO") || album.metadata.title == "My Album" {
        results.warn("Album title appears to be a placeholder");
    }

    if album.metadata.artist.contains("TODO")
        || album.metadata.artist == "Artist Name"
        || album.artist.name.contains("TODO")
    {
        results.warn("Artist name appears to be a placeholder");
    }

    if album.metadata.summary.contains("TODO")
        || album.metadata.summary == "Description of this album"
    {
        results.warn("Album summary is a placeholder - consider adding a description");
    }

    if album.artist.rss_author_email.contains("example.com") {
        results.warn("RSS author email is a placeholder - update for RSS feed");
    }

    if album.site.domain.contains("example.com") {
        results.warn("Site domain is a placeholder - update before deployment");
    }

    // Check for empty tracks
    if album.tracks.is_empty() {
        results.error("No tracks defined in album.toml");
    }

    println!("  ✓ Metadata structure valid");
}

fn validate_directories(path: &Path, results: &mut ValidationResults) {
    println!("📁 Validating directory structure...");

    let required_dirs = ["audio", "artwork", "notes"];
    for dir in required_dirs {
        let dir_path = path.join(dir);
        if !dir_path.exists() {
            results.error(format!("Required directory missing: {}/", dir));
        } else if !dir_path.is_dir() {
            results.error(format!("{} exists but is not a directory", dir));
        }
    }

    println!("  ✓ Directory structure valid");
}

fn validate_audio_files(
    base_path: &Path,
    album: &release_kit_core::types::Album,
    results: &mut ValidationResults,
) {
    println!("🎵 Validating audio files...");

    for (i, track) in album.tracks.iter().enumerate() {
        let track_num = i + 1;
        let audio_path = base_path.join(&track.file);

        // Check file exists
        if !audio_path.exists() {
            results.error(format!(
                "Track {} audio file not found: {}",
                track_num,
                track.file.display()
            ));
            continue;
        }

        // Check file is readable and valid audio
        match Probe::open(&audio_path) {
            Ok(probe) => match probe.read() {
                Ok(tagged_file) => {
                    let properties = tagged_file.properties();
                    let duration = properties.duration();

                    // Warn if very short (likely error)
                    if duration.as_secs() < 1 {
                        results.warn(format!(
                            "Track {} ({}) is very short ({}s) - is this correct?",
                            track_num,
                            track.title,
                            duration.as_secs()
                        ));
                    }

                    // Check duration matches if specified in config
                    if let Some(config_duration) = track.duration {
                        let actual_secs = duration.as_secs();
                        let config_secs = config_duration.as_secs();
                        if actual_secs != config_secs {
                            results.warn(format!(
                                "Track {} duration mismatch: config says {}:{:02}, file is {}:{:02}",
                                track_num,
                                config_secs / 60,
                                config_secs % 60,
                                actual_secs / 60,
                                actual_secs % 60
                            ));
                        }
                    }
                }
                Err(e) => {
                    results.error(format!(
                        "Track {} ({}) is not a valid audio file: {}",
                        track_num,
                        track.file.display(),
                        e
                    ));
                }
            },
            Err(e) => {
                results.error(format!(
                    "Track {} ({}) cannot be opened: {}",
                    track_num,
                    track.file.display(),
                    e
                ));
            }
        }
    }

    println!("  ✓ Audio files validated ({} tracks)", album.tracks.len());
}

fn validate_cover_art(base_path: &Path, results: &mut ValidationResults) {
    println!("🎨 Validating artwork...");

    let artwork_dir = base_path.join("artwork");
    let cover_names = [
        "cover.jpg",
        "cover.png",
        "cover.jpeg",
        "artwork.jpg",
        "artwork.png",
    ];

    let has_cover = cover_names
        .iter()
        .any(|name| artwork_dir.join(name).exists());

    if !has_cover {
        // Check if any image exists
        if let Ok(entries) = std::fs::read_dir(&artwork_dir) {
            let has_any_image = entries.flatten().any(|entry| {
                if let Some(ext) = entry.path().extension() {
                    let ext_lower = ext.to_string_lossy().to_lowercase();
                    ext_lower == "jpg" || ext_lower == "jpeg" || ext_lower == "png"
                } else {
                    false
                }
            });

            if !has_any_image {
                results.warn("No cover art found in artwork/ - add cover.jpg or cover.png");
            } else {
                results.warn("Cover art found but not using standard name (cover.jpg/cover.png)");
            }
        } else {
            results.warn("Cannot read artwork directory");
        }
    } else {
        println!("  ✓ Cover art found");
    }
}

fn validate_liner_notes(
    base_path: &Path,
    album: &release_kit_core::types::Album,
    results: &mut ValidationResults,
) {
    println!("📝 Validating liner notes...");

    let mut notes_checked = 0;

    // Check album liner notes if specified
    if let Some(ref liner_notes_path) = album.metadata.liner_notes {
        let full_path = base_path.join(liner_notes_path);
        if !full_path.exists() {
            results.error(format!(
                "Album liner notes file not found: {}",
                liner_notes_path.display()
            ));
        } else {
            notes_checked += 1;
        }
    }

    // Check per-track liner notes if specified
    for (i, track) in album.tracks.iter().enumerate() {
        if let Some(ref track_notes_path) = track.liner_notes {
            let full_path = base_path.join(track_notes_path);
            if !full_path.exists() {
                results.error(format!(
                    "Track {} liner notes file not found: {}",
                    i + 1,
                    track_notes_path.display()
                ));
            } else {
                notes_checked += 1;
            }
        }
    }

    if notes_checked > 0 {
        println!("  ✓ Liner notes validated ({} files)", notes_checked);
    } else {
        println!("  ⚠ No liner notes configured (optional)");
    }
}

fn print_results(results: &ValidationResults) {
    println!();

    if !results.warnings.is_empty() {
        println!("⚠️  Warnings ({}):", results.warnings.len());
        for warning in &results.warnings {
            println!("  • {}", warning);
        }
        println!();
    }

    if !results.errors.is_empty() {
        println!("❌ Errors ({}):", results.errors.len());
        for error in &results.errors {
            println!("  • {}", error);
        }
        println!();
    }
}
