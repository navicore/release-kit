# release-kit

A static site generator for independent musicians to create standalone album
release websites. Each album gets its own dedicated site with streaming,
downloads, and payment processing - fully self-hosted on your infrastructure.

## Philosophy

**Unbundle hosting from discovery.** Your music lives at a permanent URL you
control. Promote it anywhere: Instagram today, Bluesky tomorrow, whatever
platform has attention next year. The album site never moves, never disappears,
never changes its terms.

- **Zero platform fees** (vs. Bandcamp's 15%)
- **Full creative control** - each release is its own artistic statement
- **Predictable costs** - ~$2-5/month for modest success, artist-controlled
  limits
- **No algorithm lock-in** - works with any discovery mechanism

## Features (MVP)

- ✅ Pure static HTML generation (Leptos SSR, no JavaScript)
- ✅ Full-quality streaming with HTML5 audio
- ✅ Responsive design (mobile-first)
- ✅ Markdown liner notes (album + per-track, unlimited length)
- ✅ RSS feeds for community aggregation
- ✅ Cloudflare deployment (R2 + Pages + Workers)
- ✅ Rate limiting & bandwidth caps
- ✅ Payment UI stubs (functional in Phase 2)

## Quick Start

**Ideal workflow** - Point at audio files, get a working site:

```bash
# Install (coming soon)
cargo install release-kit

# You have a directory with audio files
my-album/
├── 01-infrastructure-hum.flac
├── 02-resonant-decay.flac
├── 03-harmonic-collapse.flac
└── cover.jpg

# Run init - auto-detects audio, generates config
release-kit init my-album/

# Generated structure:
my-album/
├── album.toml           # Auto-generated with smart defaults
├── artwork/
│   └── cover.jpg        # Moved here
├── audio/               # Audio files organized here
└── notes/
    └── album.md         # Template

# Edit album.toml to set artist name, release date, etc.
vim my-album/album.toml

# Preview immediately (works with defaults)
release-kit preview my-album/
# → http://localhost:8080

# Deploy to Cloudflare
export CLOUDFLARE_API_TOKEN=your-token
release-kit deploy my-album/ --target cloudflare
```

See [docs/init-command.md](docs/init-command.md) for init command details.

## Project Structure

``` my-album/ ├── album.toml              # Album metadata & configuration ├──
artwork/ │   ├── cover.jpg           # Required: album cover (3000x3000) │   └──
banner.jpg          # Optional: hero image ├── audio/ │   ├── 01-track-name.flac
│   ├── 02-another.flac │   └── ... └── notes/ ├── album.md            # Album
liner notes ├── track-01.md         # Per-track notes (optional) └── track-03.md
```

## album.toml Example

```toml [album] title = "Concrete Frequencies" artist = "Your Name" release_date
= "2025-11-15" summary = "Urban soundscapes through granular synthesis" genre =
["experimental", "electroacoustic"] license = "CC BY-NC-SA 4.0" liner_notes =
"notes/album.md"

[artist] name = "Your Name" rss_author_email = "you@example.com"

[site] domain = "concrete-frequencies.example.com" theme = "default"
accent_color = "#ff6b35"

[[track]] file = "audio/01-infrastructure-hum.flac" title = "Infrastructure Hum"
liner_notes = "notes/track-01.md"

[[track]] file = "audio/02-resonant-decay.flac" title = "Resonant Decay"

[distribution] streaming_enabled = true download_enabled = false  # Coming soon

[hosting.cloudflare] account_id = "your-account-id" r2_bucket = "music-releases"

[limits] max_monthly_bandwidth_gb = 100

[rss] enabled = true ```

See [docs/design.md](docs/design.md) for complete specification.

## Architecture

Built with Rust + Leptos for architectural discipline:
- **Pure SSR** - No single-page app patterns, static HTML only
- **Type-safe Worker** - Rust compiled to WASM for streaming proxy
- **Direct API calls** - No SDK dependencies, full control
- **Mobile-first CSS** - Semantic HTML, CSS Grid, responsive

See [docs/architecture.md](docs/architecture.md) for technical details.

## Cost Estimate

Cloudflare hosting costs (2024):
- **1,000 streams/month:** ~$0.75
- **10,000 streams/month:** ~$7.50

Artist controls bandwidth limits to prevent surprise bills.

Compare to:
- **Bandcamp:** 15% of sales
- **Spotify:** $0.003-0.005 per stream (after cuts)
- **release-kit:** ~$0.0001 per stream

## Development Status

🚧 **Phase 1 (current):** MVP implementation
- [x] Workspace setup & core types
- [x] Config parsing & validation
- [x] Rust edition 2024 & `just` tooling
- [ ] Static site generation (Leptos)
- [ ] Cloudflare Worker implementation
- [ ] Cloudflare deployment
- [ ] CLI commands (full implementation)

📋 **Phase 2:** Payments & federation
- Stripe integration
- Download generation & delivery
- ActivityPub support
- Audio transcoding

🔮 **Phase 3:** Community & analytics
- Aggregator/directory
- Privacy-respecting analytics
- Email collection
- Merch integration

## Contributing

This project is in early development. Design discussions and architecture
feedback welcome!

## Development

This project uses [`just`](https://github.com/casey/just) as the command runner
to ensure local development and CI/CD use identical commands.

### Prerequisites

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install just
cargo install just

# Optional: Install shell completions for better CLI experience
just install-completions-bash  # or -zsh, -fish
```

### Common Commands

```bash # Show all available commands just

# Build the project just build

# Run tests just test

# Format code just fmt

# Run clippy lints just lint

# Run all pre-commit checks (format, lint, test) just pre-commit

# Run full CI checks (what GitHub Actions runs) just ci

# Validate example album just validate-example

# Build release version just build-release ```

**Important:** Always use `just` commands instead of `cargo` directly. This
ensures your local environment matches CI/CD exactly.

### Shell Completions

Tab completion for the CLI makes development faster:

```bash
# Generate completions for your shell
cargo run -- completions bash   # or zsh, fish, powershell, elvish

# Install with just (recommended)
just install-completions-bash    # Installs to ~/.local/share/bash-completion/
just install-completions-zsh     # Installs to ~/.zsh/completions/
just install-completions-fish    # Installs to ~/.config/fish/completions/
```

After installing, restart your shell or source the completion file.

### GitHub Actions

All CI/CD workflows use `just` to run the same commands you use locally. See
`.github/workflows/ci.yml`.

## Documentation

- [Design Document](docs/design.md) - Product vision, features, UX
- [Architecture](docs/architecture.md) - Technical implementation details

## License

MIT License - see [LICENSE](LICENSE)

## Author

Ed Sweeney ([@navicore](https://github.com/navicore))

Experimental electroacoustic musician and software developer. Building tools for
the indie music community.
