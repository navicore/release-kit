// Cloudflare API client
// TODO: Implement R2, Pages, and Workers API calls

#[allow(dead_code)] // MVP: Implementation pending
pub struct CloudflareClient {
    account_id: String,
    api_token: String,
}

impl CloudflareClient {
    pub fn new(account_id: String, api_token: String) -> Self {
        Self {
            account_id,
            api_token,
        }
    }
}
