use std::path::PathBuf;

pub async fn run(path: PathBuf, output: PathBuf) -> anyhow::Result<()> {
    println!("Building site from: {}", path.display());
    println!("Output to: {}", output.display());
    println!("TODO: Generate static site with Leptos SSR");
    Ok(())
}
