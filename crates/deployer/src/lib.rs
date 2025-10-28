// Deployment targets (Cloudflare, future: Netlify, static)
// TODO: Implement Cloudflare API client, R2 upload, Pages deployment, Worker deployment

pub mod cloudflare;

use async_trait::async_trait;

pub struct DeploymentResult {
    pub site_url: String,
    pub feed_url: String,
}

#[async_trait]
pub trait Deployer {
    async fn deploy(&self) -> anyhow::Result<DeploymentResult>;
}
