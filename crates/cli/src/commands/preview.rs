use std::path::PathBuf;

pub async fn run(path: PathBuf, port: u16) -> anyhow::Result<()> {
    println!("Starting preview server at: {}", path.display());
    println!("Port: {}", port);
    println!("TODO: Build site and serve with hot reload");
    println!("Preview: http://localhost:{}", port);
    Ok(())
}
