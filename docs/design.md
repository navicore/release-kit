# release-kit: Design Document

## Project Overview

**release-kit** is a static site generator for independent musicians to create standalone album release websites. Each album gets its own dedicated site with streaming, optional downloads, and payment processing - fully self-hosted on the artist's infrastructure.

### Core Philosophy

**Unbundle hosting from discovery:**
- Streaming/hosting is commodity infrastructure (cheap, reliable)
- Discovery/promotion happens on whatever social platform has attention (Instagram, Bluesky, Mastodon, etc.)
- The album site is the canonical URL - platforms come and go, the music stays permanent
- Artist controls everything: hosting costs, branding, data, user experience

**Platform-agnostic promotion:**
```
Instagram today     }
Bluesky tomorrow    } → Same URL, artist in control
ActivityPub 2026    }
Future platform     }
```

### Target Audience

Experimental/electroacoustic musicians who:
- Value artistic control and aesthetic consistency
- Are part of active communities with their own discovery mechanisms
- Want to avoid platform lock-in (Spotify, Bandcamp)
- Are comfortable with basic technical tools (CLI, config files)
- Want to minimize fees (Bandcamp takes 15%, this takes ~3% for Stripe)

## Technical Stack

### Core Technologies

- **Rust** - Generator CLI and build tooling
- **Leptos** - Static site generation (SSR only, no client hydration)
- **Cloudflare** (MVP target):
  - R2 for audio file storage ($0.015/GB egress)
  - Workers (Rust → WASM) for streaming proxy, rate limiting
  - Pages for static site hosting
  - KV/D1 for metadata, future purchase records
- **Future targets:** Netlify, AWS, self-hosted static files

### Why This Stack

- **Rust + Leptos:** Type-safe, generates optimized static HTML, architectural discipline
- **No client-side JavaScript:** Pure SSR, HTML5 native controls, fast page loads
- **Cloudflare:** Generous free tier, cheap bandwidth, integrated CDN, simple deployment
- **Static-first:** Fast, cacheable, works without JavaScript, accessible

### Architectural Principles

1. **No Single-Page App patterns** - Static HTML, server-side rendering only
2. **Pure Leptos** - No mixing with vanilla JavaScript that undermines architecture
3. **HTML5 native controls** - Browser audio player, no custom JS controls for MVP
4. **Responsive from day one** - Mobile-first CSS, semantic HTML
5. **Type safety everywhere** - Rust Worker shares types with generator

## Input Format

### Directory Structure

```
my-album/
├── album.toml              # All metadata and configuration
├── artwork/
│   ├── cover.jpg           # Required (recommend 3000x3000)
│   └── banner.jpg          # Optional hero image
├── audio/
│   ├── 01-track-name.flac
│   ├── 02-another-track.flac
│   └── ...
└── notes/
    ├── album.md            # Album-level liner notes
    ├── track-01.md         # Per-track liner notes (optional)
    └── track-03.md         # Can have notes for some tracks, not others
```

### album.toml Schema

```toml
[album]
title = "Album Title"
artist = "Artist Name"
release_date = "2025-11-15"
summary = "Short description for RSS/social sharing"
genre = ["experimental", "electroacoustic", "drone"]
catalog_number = "YN-001"  # Optional
license = "CC BY-NC-SA 4.0"  # Or "All Rights Reserved"
liner_notes = "notes/album.md"  # Path to markdown file

[artist]
name = "Artist Name"
url = "https://artist-main-site.com"  # Optional
bio = "Artist biography..."
rss_author_email = "artist@example.com"

[site]
domain = "album-name.example.com"
theme = "default"  # MVP: only "default" available
accent_color = "#ff6b35"  # Theme can use this for highlights

# Every track explicitly defined (TOML is source of truth)
[[track]]
file = "audio/01-track-name.flac"
title = "Track Title"
duration = "7:23"  # Optional - auto-detect from file if omitted
liner_notes = "notes/track-01.md"  # Optional - path to markdown

[[track]]
file = "audio/02-another-track.flac"
title = "Another Track"
# No duration - will auto-detect
# No liner_notes - that's fine

[distribution]
streaming_enabled = true  # Always full quality, no transcoding in MVP

# MVP: UI stubs only ("Coming Soon" in generated site)
download_enabled = false
download_price = 7.00
pay_what_you_want = false
tip_jar_enabled = false
tip_suggested_amounts = [3, 5, 10]
download_formats = ["flac", "mp3-320"]  # Shows in UI even if disabled

[hosting.cloudflare]
account_id = "your-cloudflare-account-id"
# API token read from CLOUDFLARE_API_TOKEN env var
r2_bucket = "music-releases"
pages_project = "album-project-name"

[limits]
max_monthly_bandwidth_gb = 100
max_concurrent_streams = 50

[rss]
enabled = true  # Generates feed at /feed.xml
```

### Design Principles

1. **TOML is source of truth** - don't read ID3/Vorbis tags in MVP (future enhancement)
2. **Explicit track definitions** - no auto-discovery from filenames, user declares everything
3. **Markdown for prose** - Album and per-track liner notes, no length limits, full CommonMark
4. **Optional is really optional** - Track liner notes, banner image, duration (auto-detect), etc.
5. **Theme-agnostic config** - TOML never has theme-specific settings; themes interpret generic fields

## CLI Design

### Commands

```bash
release-kit init my-album/
# Smart init: scans directory for audio files, auto-detects metadata
# Generates album.toml with sensible defaults, creates directory structure
# See docs/init-command.md for detailed behavior

release-kit validate my-album/
# Lints configuration, verifies files exist, checks audio metadata
# Reports warnings (missing optional fields) and errors (missing required files)

release-kit preview my-album/
# Runs local dev server (http://localhost:8080) with rebuild on file change

release-kit build my-album/ --output dist/
# Generates static site + Worker code (no deployment)

release-kit deploy my-album/ --target cloudflare
# Validates, builds, and deploys to Cloudflare
# Future: --target netlify, --target static (just files)

release-kit completions <SHELL>
# Generate shell completion scripts (bash, zsh, fish, powershell, elvish)
```

### Validation & Linting

**Philosophy:** Be helpful but let users make mistakes. Warn liberally, error only for showstoppers.

**Example output:**
```bash
$ release-kit validate my-album/

✓ album.toml valid
✓ Cover artwork: artwork/cover.jpg (3000x3000 JPEG)
⚠ Banner image missing (optional, but recommended)
✓ Track 1: audio/01-hum.flac (7:24 duration, FLAC 24/48kHz)
  ⚠ Duration mismatch: TOML says 7:23, file is 7:24
  ✓ Liner notes: notes/track-01.md (847 words)
✓ Track 2: audio/02-decay.flac (5:47 duration, FLAC 24/48kHz)
  ℹ No liner notes (optional)
✓ Album liner notes: notes/album.md (312 words)

Summary: 2 tracks, 13:11 total, 487 MB
Warnings: 2 (non-blocking)
```

**Categories:**
- ✓ Success
- ⚠ Warning (continues anyway)
- ℹ Info (just FYI)
- ✗ Error (blocks deployment)

### Deployment Flow

```bash
$ release-kit deploy my-album/ --target cloudflare

[1/4] Validating...
✓ Configuration valid
✓ All files present

[2/4] Building site...
✓ Generated static HTML/CSS
✓ Compiled Worker to WASM
✓ Generated RSS feed

[3/4] Uploading to Cloudflare...
→ Creating R2 bucket 'music-releases' (or using existing)
→ Uploading 2 audio files (487 MB)... [████████] 100%
→ Deploying Cloudflare Pages...
→ Deploying Worker (streaming proxy)...

[4/4] Finalizing...
✓ Deployed successfully!

URLs:
  Site: https://my-album.pages.dev
  Custom domain: https://album.example.com (configure DNS manually)
  RSS feed: https://album.example.com/feed.xml

Next steps:
  1. Point album.example.com DNS to Cloudflare Pages
  2. Share the RSS feed with your community
  3. Promote on social media
```

## MVP Scope

### Included in MVP

✅ **Core functionality:**
- Parse album.toml and validate
- Generate static site with Leptos SSR (no client hydration)
- HTML5 audio player (full quality streaming, native controls)
- Render markdown liner notes (album + per-track)
- RSS feed generation
- Deploy to Cloudflare (R2 + Pages + Worker)
- Rate limiting and bandwidth caps
- Responsive design (mobile-friendly)
- Rust Worker (WASM) with type safety

✅ **Default theme:**
- Single theme: "default"
- Minimalist, Bandcamp-inspired aesthetic
- Dark theme with configurable accent color
- Accessible, semantic HTML
- CSS Grid, mobile-first

✅ **Payment UI stubs:**
- "Download" button (greyed out, "Coming Soon")
- "Tip Jar" widget (greyed out, "Coming Soon")
- TOML configuration exists but doesn't activate features

### Deferred (Post-MVP)

⏸️ **Phase 2:**
- Payment processing (Stripe integration)
- Actual download generation and delivery
- ActivityPub integration (album as Fediverse actor)
- Bluesky/ATProto structured data
- Audio transcoding (FLAC → MP3)
- Multiple theme support
- Light theme variant

⏸️ **Phase 3:**
- Community aggregator/directory
- Analytics dashboard
- Email collection for mailing list
- Merch/physical media integration

## Theme System (Future)

### MVP: Default Theme Only

**Characteristics:**
- Minimalist, Bandcamp-inspired
- Dark background with configurable accent color
- Typography: system font stack
- CSS Grid-based layout
- Mobile-first responsive
- Zero JavaScript

**What it renders:**
- Cover artwork (hero section)
- Track list with HTML5 audio players
- Album title, artist, summary
- Individual track pages with liner notes
- Album-level liner notes page
- Payment stubs ("Coming Soon")

### Future: Theme Selection

```toml
[site]
theme = "brutalist"  # or "default", "minimal", "maximalist", custom path
```

**Design principle:** Themes interpret generic TOML fields differently:
- "brutalist" might render track list as plain text table
- "maximalist" might add CSS animations
- "minimal" might hide liner notes behind details/summary

**TOML never has theme-specific keys** - themes adapt to standard schema.

## RSS Feed

### Purpose

Enable community aggregators and individual fans to monitor new releases from artists. One feed per album site.

### Content

```xml
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
  <channel>
    <title>Album Title - Artist Name</title>
    <link>https://album.example.com</link>
    <description>Album summary...</description>
    <atom:link href="https://album.example.com/feed.xml" rel="self" type="application/rss+xml"/>

    <!-- Single item: album release -->
    <item>
      <title>Album Title Released</title>
      <link>https://album.example.com</link>
      <description>New album: 8 tracks, 42:17. Stream now.</description>
      <pubDate>Fri, 15 Nov 2025 00:00:00 GMT</pubDate>
      <guid>https://album.example.com</guid>
      <enclosure url="https://album.example.com/artwork/cover.jpg" type="image/jpeg"/>
    </item>
  </channel>
</rss>
```

**Future:** Could add items for updates (remaster, bonus tracks, liner note revisions).

## Cost Model

### Cloudflare Pricing (2024)

- **R2 Storage:** $0.015/GB/month stored
- **R2 Egress:** $0.00 to Cloudflare Workers
- **Workers:** 100k requests/day free, then $0.50/million
- **Pages:** Free for unlimited sites

### Realistic Costs

**Modest success (1000 streams/month):**
- 1000 streams × 50 MB average album = 50 GB
- Cost: ~$0.75/month

**Viral success (10k streams/month):**
- 10k streams × 50 MB = 500 GB
- Cost: ~$7.50/month

**With downloads (100 FLAC downloads @ 500 MB each):**
- Additional 50 GB
- Cost: +$0.75/month

**Artist control:**
- Set `max_monthly_bandwidth_gb = 100` in config
- Worker enforces limit, returns 429 (rate limited) when exceeded
- Prevents surprise bills

**Comparison:**
- Bandcamp: 15% of sales + transaction fees
- Spotify: $0.003-0.005 per stream to artist (after label/distributor cuts)
- release-kit: ~$0.0001 per stream + Stripe 2.9% (when enabled)

## Success Criteria

MVP is successful if:
1. ✅ Artist can deploy a functional album site in < 30 minutes
2. ✅ Site is fully responsive and accessible
3. ✅ Streaming works on all modern browsers
4. ✅ Cost stays predictable and low
5. ✅ Artist has full control over content and presentation
6. ✅ Generated sites work without JavaScript
7. ✅ RSS feed enables community aggregation

---

**Document Status:** Design complete, implementation in progress
**Last Updated:** 2025-10-28
**Next Step:** Complete MVP implementation

## Related Documentation

- [Architecture](architecture.md) - Technical implementation details
- [Init Command](init-command.md) - Smart initialization workflow
- [Quick Start](../README.md#quick-start) - Get started quickly
