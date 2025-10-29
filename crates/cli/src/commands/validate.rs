use release_kit_core::parse_album_toml;
use std::path::PathBuf;

pub async fn run(path: PathBuf) -> anyhow::Result<()> {
    println!("Validating album at: {}", path.display());

    let config_path = path.join("album.toml");
    let album = parse_album_toml(&config_path)?;

    println!("✓ album.toml valid");
    println!("  Album: {} by {}", album.metadata.title, album.artist.name);
    println!("  Tracks: {}", album.tracks.len());

    println!("\nTODO: Full validation (files, audio metadata, images)");

    Ok(())
}
