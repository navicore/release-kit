// Static site generation with Leptos SSR
// TODO: Implement Leptos components, CSS, Worker builder, RSS generation

pub mod components;

pub struct GeneratedSite {
    pub pages: Vec<(String, String)>,   // (path, html)
    pub assets: Vec<(String, Vec<u8>)>, // (path, data)
}

pub fn generate_site() -> GeneratedSite {
    GeneratedSite {
        pages: vec![],
        assets: vec![],
    }
}
