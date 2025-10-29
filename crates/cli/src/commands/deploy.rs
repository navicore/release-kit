use crate::DeployTarget;
use std::path::PathBuf;

pub async fn run(path: PathBuf, target: DeployTarget) -> anyhow::Result<()> {
    println!("Deploying album from: {}", path.display());
    println!("Target: {:?}", target);
    println!("TODO: Validate, build, and deploy to Cloudflare");
    Ok(())
}
