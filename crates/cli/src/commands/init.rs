use std::path::PathBuf;

pub async fn run(path: PathBuf) -> anyhow::Result<()> {
    println!("Initializing album directory at: {}", path.display());
    println!("TODO: Create directory structure and template album.toml");
    Ok(())
}
