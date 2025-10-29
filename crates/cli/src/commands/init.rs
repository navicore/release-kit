use anyhow::{Context, Result};
use chrono::Local;
use lofty::prelude::*;
use lofty::probe::Probe;
use std::fs;
use std::path::{Path, PathBuf};
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

#[derive(Debug)]
struct DetectedTrack {
    path: PathBuf,
    title: String,
    duration: Option<String>,
    #[allow(dead_code)] // Will be used in future for format-specific handling
    format: String,
}

pub async fn run(path: PathBuf) -> Result<()> {
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
    generate_album_toml(&path, &tracks)?;

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

fn scan_audio_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut audio_files = Vec::new();

    for entry in WalkDir::new(dir)
        .max_depth(2)
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

fn extract_track_title(path: &Path, track_number: usize) -> String {
    let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Track");

    // Remove common track number prefixes
    let cleaned = filename
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '-' || c == '_' || c == '.')
        .trim_start_matches("track")
        .trim_start_matches('-')
        .trim_start_matches('_')
        .trim();

    if cleaned.is_empty() {
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
    generate_album_toml(base, &[])?;
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
        if audio_file.canonicalize()? == dest.canonicalize().unwrap_or(dest.clone()) {
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
        if cover_path.canonicalize()? != dest.canonicalize().unwrap_or(dest.clone()) {
            fs::copy(cover_path, &dest).context("Failed to copy cover art")?;
        }
    }

    Ok(())
}

fn generate_album_toml(base: &Path, tracks: &[DetectedTrack]) -> Result<()> {
    let today = Local::now().format("%Y-%m-%d").to_string();

    let mut toml = format!(
        r##"# Generated by release-kit init
# Edit this file to customize your album

[album]
title = "My Album"  # TODO: Set album title
artist = "Artist Name"  # TODO: Set artist name
release_date = "{}"  # TODO: Set release date
summary = "Description of this album"  # TODO: Add summary
genre = ["experimental"]  # TODO: Set genres
license = "CC BY-NC-SA 4.0"
liner_notes = "notes/album.md"

[artist]
name = "Artist Name"  # TODO: Set artist name
rss_author_email = "artist@example.com"  # TODO: Set email

[site]
domain = "my-album.example.com"  # TODO: Set domain
theme = "default"
accent_color = "#ff6b35"

"##,
        today
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
            let filename = track.path.file_name().unwrap().to_string_lossy();
            toml.push_str("[[track]]\n");
            toml.push_str(&format!("file = \"audio/{}\"\n", filename));
            toml.push_str(&format!("title = \"{}\"\n", track.title));
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
account_id = "your-cloudflare-account-id"  # TODO: Set from env or config
r2_bucket = "music-releases"
pages_project = "my-album"

[rss]
enabled = true
"##,
    );

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
