use anyhow::{Context, Result};
use release_kit_core::config::parse_album_toml;
use std::fs;
use std::path::PathBuf;

use super::template::{detect_cover_art, generate_html, generate_player_js};

/// Build static site for deployment
pub async fn run(path: PathBuf, output: PathBuf) -> Result<()> {
    println!("🔨 Building static site...");
    println!("   Source: {}", path.display());
    println!("   Output: {}", output.display());
    println!();

    // Validate album directory exists
    if !path.exists() {
        anyhow::bail!("Album directory does not exist: {}", path.display());
    }

    // Load and validate album.toml
    let album_toml_path = path.join("album.toml");
    if !album_toml_path.exists() {
        anyhow::bail!(
            "album.toml not found in {}\\nRun 'release-kit init {}' first",
            path.display(),
            path.display()
        );
    }

    let album = parse_album_toml(&album_toml_path).context("Failed to parse album.toml")?;

    println!("✓ Loaded: {}", album.metadata.title);
    println!("  Artist: {}", album.metadata.artist);
    println!("  Tracks: {}", album.tracks.len());
    println!();

    // Create output directory structure
    println!("📁 Creating output directory structure...");
    fs::create_dir_all(&output).context("Failed to create output directory")?;
    fs::create_dir_all(output.join("audio")).context("Failed to create audio directory")?;
    fs::create_dir_all(output.join("artwork")).context("Failed to create artwork directory")?;
    fs::create_dir_all(output.join("notes")).context("Failed to create notes directory")?;
    println!("   ✓ Created directories");

    // Copy audio files
    println!("🎵 Copying audio files...");
    let mut copied_audio = 0;
    for track in &album.tracks {
        let src = path.join(&track.file);
        let filename = track.file.file_name().context("Invalid track filename")?;
        let dst = output.join("audio").join(filename);

        if src.exists() {
            fs::copy(&src, &dst).with_context(|| format!("Failed to copy {}", src.display()))?;
            copied_audio += 1;
        } else {
            eprintln!("   ⚠ Warning: Audio file not found: {}", src.display());
        }
    }
    println!("   ✓ Copied {} audio files", copied_audio);

    // Copy artwork
    println!("🎨 Copying artwork...");
    let artwork_src = path.join("artwork");
    let mut copied_artwork = 0;
    if artwork_src.exists() {
        for entry in fs::read_dir(&artwork_src)? {
            let entry = entry?;
            let src_path = entry.path();
            if src_path.is_file() {
                let filename = src_path.file_name().unwrap();
                let dst_path = output.join("artwork").join(filename);
                fs::copy(&src_path, &dst_path)?;
                copied_artwork += 1;
            }
        }
    }
    println!("   ✓ Copied {} artwork files", copied_artwork);

    // Copy liner notes
    println!("📝 Copying liner notes...");
    let notes_src = path.join("notes");
    let mut copied_notes = 0;
    if notes_src.exists() {
        for entry in fs::read_dir(&notes_src)? {
            let entry = entry?;
            let src_path = entry.path();
            if src_path.is_file() {
                let filename = src_path.file_name().unwrap();
                let dst_path = output.join("notes").join(filename);
                fs::copy(&src_path, &dst_path)?;
                copied_notes += 1;
            }
        }
    }
    println!("   ✓ Copied {} liner note files", copied_notes);

    // Generate index.html
    println!("📄 Generating index.html...");
    let cover_art = detect_cover_art(&path.join("artwork"));
    let html = generate_html(&album, cover_art.as_deref(), false);
    fs::write(output.join("index.html"), html).context("Failed to write index.html")?;
    println!("   ✓ Generated index.html");

    // Generate player.js
    println!("🎮 Generating player.js...");
    let player_js = generate_player_js();
    fs::write(output.join("player.js"), player_js).context("Failed to write player.js")?;
    println!("   ✓ Generated player.js");

    println!();
    println!("✅ Build complete!");
    println!("   Output: {}", output.display());
    println!();
    println!("To test locally:");
    println!("   cd {} && python3 -m http.server 8000", output.display());
    println!();

    Ok(())
}
