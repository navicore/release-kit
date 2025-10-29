# release-kit: Technical Architecture

## Overview

release-kit is built as a Rust workspace with multiple specialized crates. The architecture prioritizes type safety, architectural discipline (pure SSR, no SPA patterns), and clear separation of concerns.

## Core Architectural Principles

1. **Pure Leptos SSR** - No client-side hydration, no JavaScript except HTML5 audio
2. **Type safety everywhere** - Rust Worker shares types with generator
3. **Static-first** - Generate HTML at build time, not per-request
4. **Direct API calls** - No SDK dependencies, full control over Cloudflare API
5. **Responsive from day one** - Mobile-first CSS, semantic HTML

## Project Structure

```
release-kit/
├── Cargo.toml                    # Workspace root
├── LICENSE
├── README.md
├── docs/
│   ├── design.md                 # Design document
│   └── architecture.md           # This file
├── crates/
│   ├── cli/                      # Main binary (release-kit)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       └── commands/
│   │           ├── init.rs
│   │           ├── validate.rs
│   │           ├── preview.rs
│   │           ├── build.rs
│   │           └── deploy.rs
│   ├── core/                     # Shared types & config
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs          # Album, Track, Artist, etc.
│   │       ├── config.rs         # TOML parsing
│   │       └── error.rs
│   ├── validator/                # Validation logic
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config_validator.rs
│   │       ├── file_validator.rs
│   │       └── audio_validator.rs
│   ├── generator/                # Static site generation
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── components/       # Leptos components
│   │       │   ├── album_page.rs
│   │       │   ├── track_list.rs
│   │       │   ├── track_page.rs
│   │       │   └── layout.rs
│   │       ├── theme/
│   │       │   └── default/
│   │       │       └── styles.css
│   │       ├── rss.rs            # RSS feed generation
│   │       └── worker_builder.rs # Build Worker WASM
│   ├── deployer/                 # Deployment targets
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── cloudflare/
│   │       │   ├── mod.rs
│   │       │   ├── client.rs     # API client
│   │       │   ├── r2.rs         # R2 operations
│   │       │   ├── pages.rs      # Pages deployment
│   │       │   └── workers.rs    # Worker deployment
│   │       └── traits.rs         # Deployer trait (future: Netlify, etc.)
│   └── worker-template/          # Cloudflare Worker
│       ├── Cargo.toml            # Separate WASM target
│       └── src/
│           └── lib.rs
└── examples/
    └── test-album/               # Example album for testing
        ├── album.toml
        ├── audio/
        ├── artwork/
        └── notes/
```

## Crate Responsibilities

### `crates/core`

**Purpose:** Shared types and configuration parsing used across all crates.

**Key Types:**
```rust
pub struct Album {
    pub metadata: AlbumMetadata,
    pub artist: Artist,
    pub tracks: Vec<Track>,
    pub artwork: Artwork,
    pub distribution: Distribution,
    pub hosting: HostingConfig,
    pub rss: RssConfig,
}

pub struct Track {
    pub file: PathBuf,
    pub title: String,
    pub duration: Option<Duration>,
    pub liner_notes: Option<PathBuf>,
}

pub struct AlbumMetadata {
    pub title: String,
    pub artist: String,
    pub release_date: NaiveDate,
    pub summary: String,
    pub genre: Vec<String>,
    pub catalog_number: Option<String>,
    pub license: String,
    pub liner_notes: Option<PathBuf>,
}
```

**Responsibilities:**
- Parse `album.toml` with serde
- Provide validated, type-safe config to other crates
- Define error types used project-wide

**Dependencies:**
- `serde`, `toml` - Config parsing
- `chrono` - Date handling
- `anyhow` - Error handling

### `crates/validator`

**Purpose:** Validate album configuration and files before build/deploy.

**Validation Categories:**
1. **Config validation** - TOML structure, required fields
2. **File validation** - Paths exist, correct formats
3. **Audio validation** - Duration detection, format checking
4. **Cross-checks** - Duration mismatches, missing liner notes (warnings)

**Output:**
```rust
pub struct ValidationReport {
    pub errors: Vec<ValidationError>,    // Block deployment
    pub warnings: Vec<ValidationWarning>, // Show but continue
    pub info: Vec<ValidationInfo>,        // Just FYI
}
```

**Dependencies:**
- `lofty` - Audio metadata detection
- `image` - Image validation (dimensions, format)
- `walkdir` - Directory traversal

### `crates/generator`

**Purpose:** Generate static HTML, CSS, and Worker code from album config.

**Process:**
1. Load album config from `core`
2. Render Leptos components to static HTML (SSR only)
3. Compile Worker crate to WASM
4. Generate RSS feed
5. Output bundle ready for deployment

**Leptos Components:**
```rust
#[component]
fn AlbumPage(album: Album) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <title>{&album.metadata.title} " - " {&album.artist.name}</title>
                <style>{include_str!("theme/default/styles.css")}</style>
            </head>
            <body>
                <Header album=album.clone()/>
                <TrackList tracks=album.tracks.clone()/>
                <Footer album=album.clone()/>
            </body>
        </html>
    }
}

#[component]
fn TrackList(tracks: Vec<Track>) -> impl IntoView {
    view! {
        <ol class="track-list">
            {tracks.into_iter().map(|track| view! {
                <li class="track">
                    <audio
                        controls
                        preload="none"
                        src={format!("/stream/{}", track.file_name())}
                    >
                        "Your browser does not support audio playback."
                    </audio>
                    <span class="track-title">{track.title}</span>
                    <span class="track-duration">{format_duration(track.duration)}</span>
                </li>
            }).collect::<Vec<_>>()}
        </ol>
    }
}
```

**CSS Strategy:**
- Single embedded stylesheet (no external CSS in MVP)
- Mobile-first, semantic HTML
- CSS Grid for layouts
- CSS custom properties for theming
- Zero JavaScript

**Worker Build:**
```rust
pub fn build_worker() -> Result<Vec<u8>> {
    // Compile worker-template crate to WASM
    Command::new("cargo")
        .args(&[
            "build",
            "--target", "wasm32-unknown-unknown",
            "--release",
            "--manifest-path", "crates/worker-template/Cargo.toml"
        ])
        .status()?;

    // Read compiled WASM
    std::fs::read("crates/worker-template/target/wasm32-unknown-unknown/release/worker_template.wasm")
}
```

**Dependencies:**
- `leptos` (SSR features only)
- `pulldown-cmark` - Markdown rendering
- `chrono` - RSS date formatting

### `crates/deployer`

**Purpose:** Deploy generated site to hosting platforms (Cloudflare MVP, others later).

**Design Pattern:**
```rust
#[async_trait]
pub trait Deployer {
    async fn deploy(
        &self,
        site: &GeneratedSite,
        config: &DeployConfig
    ) -> Result<DeploymentResult>;
}

pub struct CloudflareDeployer {
    client: CloudflareClient,
}

impl Deployer for CloudflareDeployer {
    async fn deploy(&self, site: &GeneratedSite, config: &DeployConfig) -> Result<DeploymentResult> {
        // 1. Create/verify R2 bucket
        // 2. Upload audio files to R2
        // 3. Deploy Worker WASM
        // 4. Deploy Pages site
        // 5. Return URLs
    }
}
```

**Cloudflare API Client** (no SDK, direct REST calls):
```rust
pub struct CloudflareClient {
    client: reqwest::Client,
    account_id: String,
    api_token: String,
}

impl CloudflareClient {
    pub async fn create_r2_bucket(&self, name: &str) -> Result<()> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/r2/buckets",
            self.account_id
        );

        let resp = self.client
            .post(&url)
            .bearer_auth(&self.api_token)
            .json(&json!({ "name": name }))
            .send()
            .await?;

        // Parse response, handle errors
    }

    pub async fn upload_to_r2(
        &self,
        bucket: &str,
        key: &str,
        data: Vec<u8>
    ) -> Result<()> {
        // S3-compatible API to R2
    }

    pub async fn deploy_worker(
        &self,
        name: &str,
        wasm: Vec<u8>
    ) -> Result<WorkerDeployment> {
        // Workers API: upload WASM module
    }

    pub async fn deploy_pages(
        &self,
        project: &str,
        files: HashMap<PathBuf, Vec<u8>>
    ) -> Result<PagesDeployment> {
        // Pages API: direct deployment
    }
}
```

**Dependencies:**
- `reqwest` - HTTP client
- `serde_json` - JSON (de)serialization
- `tokio` - Async runtime
- `async-trait` - Trait definitions

### `crates/worker-template`

**Purpose:** Cloudflare Worker (compiled to WASM) for streaming proxy and rate limiting.

**Key Responsibilities:**
1. Proxy streaming requests to R2
2. Handle HTTP Range requests (audio seeking)
3. Enforce rate limits (per-IP, global)
4. Track bandwidth usage
5. Future: Payment verification, download tokens

**Implementation:**
```rust
use worker::*;

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get_async("/stream/:track", handle_stream)
        .get_async("/download/:track", handle_download) // Future
        .run(req, env)
        .await
}

async fn handle_stream(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let track = ctx.param("track").unwrap();

    // Check rate limits
    if !check_rate_limit(&ctx, &req).await? {
        return Response::error("Rate limit exceeded", 429);
    }

    // Get R2 object
    let bucket = ctx.bucket("AUDIO_BUCKET")?;
    let object = bucket.get(track).execute().await?;

    if let Some(body) = object.body() {
        // Handle Range header for seeking
        let range = req.headers().get("Range")?;

        let mut response = Response::from_body(body)?;
        response.headers_mut().set("Content-Type", "audio/flac")?;
        response.headers_mut().set("Accept-Ranges", "bytes")?;

        Ok(response)
    } else {
        Response::error("Track not found", 404)
    }
}

async fn check_rate_limit(ctx: &RouteContext<()>, req: &Request) -> Result<bool> {
    // Use KV to track request counts per IP
    // Enforce limits from deployment config
    todo!()
}
```

**Compilation:**
- Target: `wasm32-unknown-unknown`
- Separate `Cargo.toml` from workspace (different target)
- Built by generator crate, deployed by deployer crate

**Dependencies:**
- `worker` - Cloudflare Workers SDK

### `crates/cli`

**Purpose:** Main binary, user interface, command routing.

**Commands:**
```rust
#[derive(Parser)]
#[command(name = "release-kit")]
#[command(about = "Static site generator for album releases")]
enum Command {
    /// Initialize new album directory
    Init {
        path: PathBuf,
    },

    /// Validate album configuration
    Validate {
        path: PathBuf,
    },

    /// Preview site locally
    Preview {
        path: PathBuf,
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },

    /// Build site without deploying
    Build {
        path: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Deploy site to hosting platform
    Deploy {
        path: PathBuf,
        #[arg(short, long, default_value = "cloudflare")]
        target: DeployTarget,
    },
}

#[derive(Clone, ValueEnum)]
enum DeployTarget {
    Cloudflare,
    // Future: Netlify, Static
}
```

**Preview Server:**
```rust
pub async fn preview(album_dir: PathBuf, port: u16) -> Result<()> {
    // Initial build
    let output_dir = tempdir()?;
    let site = generator::generate(&album_dir, &output_dir)?;

    // Watch for changes
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let mut watcher = notify::recommended_watcher(move |res| {
        if let Ok(_) = res {
            let _ = tx.blocking_send(());
        }
    })?;
    watcher.watch(&album_dir, RecursiveMode::Recursive)?;

    // Rebuild on change
    let rebuild_dir = album_dir.clone();
    tokio::spawn(async move {
        while let Some(_) = rx.recv().await {
            println!("Files changed, rebuilding...");
            if let Err(e) = generator::generate(&rebuild_dir, &output_dir) {
                eprintln!("Build error: {}", e);
            }
        }
    });

    // Serve files
    let app = Router::new()
        .nest_service("/", ServeDir::new(&output_dir));

    println!("Preview: http://localhost:{}", port);
    axum::Server::bind(&format!("127.0.0.1:{}", port).parse()?)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
```

**Dependencies:**
- `clap` - CLI parsing
- `tokio` - Async runtime
- `axum` - Preview server
- `notify` - File watching

## Key Dependencies

```toml
[workspace.dependencies]
# CLI & orchestration
clap = { version = "4.5", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"

# Config & serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# Site generation - SSR only
leptos = { version = "0.7", default-features = false, features = ["ssr"] }

# Audio metadata
lofty = "0.21"

# Image validation
image = "0.25"

# Markdown
pulldown-cmark = "0.12"

# HTTP client
reqwest = { version = "0.12", features = ["json", "multipart"] }

# Local preview
axum = "0.7"
tower-http = { version = "0.5", features = ["fs", "trace"] }
notify = "6"

# Utilities
walkdir = "2"
sha2 = "0.10"
chrono = "0.4"
async-trait = "0.1"

# Worker (only in worker-template)
worker = "0.4"
```

## Build Pipeline

```
User: release-kit deploy my-album/ --target cloudflare
    ↓
[CLI] Parse arguments, load environment
    ↓
[Core] Parse album.toml → Album struct
    ↓
[Validator] Check files, audio metadata
    ├─ Errors? → Exit with report
    └─ Warnings? → Show but continue
    ↓
[Generator] Create static site
    ├─ Render Leptos components to HTML
    ├─ Process markdown liner notes
    ├─ Copy artwork files
    ├─ Generate RSS feed
    ├─ Compile Worker to WASM
    └─ Bundle everything → GeneratedSite
    ↓
[Deployer] Deploy to Cloudflare
    ├─ Create/verify R2 bucket
    ├─ Upload audio files to R2 (parallel)
    ├─ Deploy Worker WASM
    ├─ Deploy Pages site (HTML/CSS/images)
    └─ Return deployment URLs
    ↓
[CLI] Print success message + URLs
```

## CSS Architecture

**Mobile-first approach:**

```css
/* Base styles (mobile) */
:root {
    --accent: #ff6b35;
    --text: #ffffff;
    --bg: #000000;
    --spacing-sm: 0.5rem;
    --spacing: 1rem;
    --spacing-lg: 2rem;
}

* {
    box-sizing: border-box;
}

body {
    font-family: system-ui, -apple-system, sans-serif;
    line-height: 1.6;
    margin: 0;
    background: var(--bg);
    color: var(--text);
}

/* Layout */
.album-header {
    display: grid;
    gap: var(--spacing);
    padding: var(--spacing);
}

.cover-art {
    width: 100%;
    aspect-ratio: 1;
    object-fit: cover;
}

.track-list {
    list-style: none;
    padding: 0;
}

.track {
    display: grid;
    gap: var(--spacing-sm);
    padding: var(--spacing);
    border-bottom: 1px solid #333;
}

.track audio {
    width: 100%;
}

/* Tablet+ */
@media (min-width: 768px) {
    .album-header {
        grid-template-columns: 1fr 2fr;
        max-width: 1200px;
        margin: 0 auto;
    }

    .track {
        grid-template-columns: auto 1fr auto;
    }
}

/* Desktop */
@media (min-width: 1024px) {
    .album-header {
        padding: var(--spacing-lg);
    }
}
```

## Testing Strategy

### Unit Tests
- Core types serialization/deserialization
- Validation logic (each validator independently)
- RSS generation
- Markdown rendering

### Integration Tests
- Full build pipeline (album.toml → GeneratedSite)
- Preview server (can serve files correctly)

### Manual Testing
- Deploy test album to Cloudflare
- Test on real devices (mobile, tablet, desktop)
- Audio playback across browsers
- RSS feed validation

## Future Enhancements

### Phase 2
- **Payment processing:** Stripe integration in Worker
- **Download generation:** Create ZIP files, time-limited URLs
- **ActivityPub:** Album as Fediverse actor
- **Transcoding:** FLAC → MP3 via FFmpeg
- **Multiple themes:** Theme selection + custom themes

### Phase 3
- **Analytics:** Privacy-respecting view/play counts
- **Community aggregator:** Discover other releases
- **Email collection:** Mailing list integration
- **CDN alternatives:** Support non-Cloudflare hosting

---

**Document Status:** Phase 1 implementation guide
**Last Updated:** 2025-10-26
**Next Step:** Workspace setup → Core types implementation
