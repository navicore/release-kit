use anyhow::{Context, Result};
use axum::{
    Router,
    extract::State,
    response::{
        Html, IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use notify::{Event as NotifyEvent, EventKind, RecursiveMode, Watcher};
use release_kit_core::config::parse_album_toml;
use std::{net::SocketAddr, path::PathBuf};
use tokio::sync::broadcast;
use tower_http::services::ServeDir;

#[derive(Clone)]
struct AppState {
    album_path: PathBuf,
    reload_tx: broadcast::Sender<()>,
}

/// Start preview server with hot reload for local development.
///
/// This command:
/// - Validates and loads album.toml
/// - Generates a simple preview HTML page
/// - Serves static files (audio, artwork)
/// - Watches for file changes and triggers hot reload
///
/// # Arguments
///
/// * `path` - Path to album directory containing album.toml
/// * `port` - Port to serve on (default: 8080)
pub async fn run(path: PathBuf, port: u16) -> Result<()> {
    println!("🎵 Starting preview server...");
    println!("   Album: {}", path.display());

    // Validate album directory exists
    if !path.exists() {
        anyhow::bail!(
            "Album directory does not exist: {}\nRun 'release-kit init {}' first",
            path.display(),
            path.display()
        );
    }

    // Load and validate album.toml
    let album_toml_path = path.join("album.toml");
    if !album_toml_path.exists() {
        anyhow::bail!(
            "album.toml not found in {}\nRun 'release-kit init {}' first",
            path.display(),
            path.display()
        );
    }

    let album = parse_album_toml(&album_toml_path).context("Failed to parse album.toml")?;

    println!("   ✓ Loaded: {}", album.metadata.title);
    println!("   ✓ Artist: {}", album.metadata.artist);
    println!("   ✓ Tracks: {}", album.tracks.len());

    // Create broadcast channel for reload events
    let (reload_tx, _) = broadcast::channel::<()>(100);

    let state = AppState {
        album_path: path.clone(),
        reload_tx: reload_tx.clone(),
    };

    // Build router
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/_reload", get(sse_handler))
        .nest_service("/audio", ServeDir::new(path.join("audio")))
        .nest_service("/artwork", ServeDir::new(path.join("artwork")))
        .with_state(state);

    // Start file watcher
    let watcher_path = path.clone();
    let watcher_tx = reload_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = watch_files(watcher_path, watcher_tx).await {
            eprintln!("File watcher error: {}", e);
        }
    });

    // Start server
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("\n🚀 Preview ready at: http://localhost:{}", port);
    println!("   Press Ctrl+C to stop\n");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind to port")?;

    axum::serve(listener, app).await.context("Server error")?;

    Ok(())
}

/// Watch for file changes and trigger reload
async fn watch_files(path: PathBuf, reload_tx: broadcast::Sender<()>) -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    let mut watcher =
        notify::recommended_watcher(move |res: Result<NotifyEvent, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.blocking_send(event);
            }
        })?;

    // Watch album directory recursively
    watcher.watch(&path, RecursiveMode::Recursive)?;

    while let Some(event) = rx.recv().await {
        match event.kind {
            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                // Filter out temporary files and hidden files
                if event.paths.iter().any(|p| {
                    let filename = p.file_name().unwrap_or_default().to_string_lossy();
                    !filename.starts_with('.') && !filename.ends_with('~')
                }) {
                    println!("   📝 File changed, reloading...");
                    let _ = reload_tx.send(());
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// SSE endpoint for hot reload
async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let mut rx = state.reload_tx.subscribe();

    let stream = async_stream::stream! {
        loop {
            if rx.recv().await.is_ok() {
                yield Ok(Event::default().data("reload"));
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Detect cover art in artwork directory
fn detect_cover_art(artwork_dir: &std::path::Path) -> Option<String> {
    const COVER_ART_NAMES: &[&str] = &[
        "cover.jpg",
        "cover.png",
        "cover.jpeg",
        "artwork.jpg",
        "artwork.png",
        "folder.jpg",
        "folder.png",
        "album.jpg",
        "album.png",
    ];

    // Try standard names first
    for name in COVER_ART_NAMES {
        let path = artwork_dir.join(name);
        if path.exists() {
            return Some(name.to_string());
        }
    }

    // Fallback: find first image file
    if let Ok(entries) = std::fs::read_dir(artwork_dir) {
        for entry in entries.flatten() {
            if let Some(ext) = entry.path().extension() {
                let ext_lower = ext.to_string_lossy().to_lowercase();
                if (ext_lower == "jpg" || ext_lower == "jpeg" || ext_lower == "png")
                    && let Some(filename) = entry.file_name().to_str()
                {
                    return Some(filename.to_string());
                }
            }
        }
    }

    None
}

/// Main index page handler
async fn index_handler(State(state): State<AppState>) -> Response {
    // Load album config
    let album_toml_path = state.album_path.join("album.toml");
    let album = match parse_album_toml(&album_toml_path) {
        Ok(a) => a,
        Err(e) => {
            return Html(format!(
                r#"<!DOCTYPE html>
<html><head><title>Error</title></head><body>
<h1>Configuration Error</h1>
<pre>{}</pre>
</body></html>"#,
                e
            ))
            .into_response();
        }
    };

    // Detect cover art
    let cover_art = detect_cover_art(&state.album_path.join("artwork"));

    // Generate simple HTML preview (will be replaced with Leptos later)
    let tracks_html: String = album
        .tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let duration = track
                .duration
                .map(|d| format!("({})", format_duration(d)))
                .unwrap_or_default();

            // Extract just the filename from the path (paths are like "audio/track.flac")
            let filename = track
                .file
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("");

            format!(
                r#"<div class="track">
                    <span class="track-number">{:02}</span>
                    <span class="track-title">{}</span>
                    <span class="track-duration">{}</span>
                    <audio controls src="/audio/{}" preload="metadata"></audio>
                </div>"#,
                i + 1,
                track.title,
                duration,
                filename
            )
        })
        .collect();

    // Generate cover art HTML if it exists
    let cover_art_html = if let Some(ref cover_filename) = cover_art {
        format!(
            r#"<img src="/artwork/{}" alt="Album cover" class="cover-art">"#,
            cover_filename
        )
    } else {
        String::new()
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{} - {} | Preview</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            line-height: 1.6;
            color: #333;
            background: #f5f5f5;
            padding: 2rem;
        }}
        .container {{
            max-width: 800px;
            margin: 0 auto;
            background: white;
            padding: 2rem;
            border-radius: 8px;
            box-shadow: 0 2px 8px rgba(0,0,0,0.1);
        }}
        .preview-badge {{
            background: #ff6b35;
            color: white;
            padding: 0.5rem 1rem;
            border-radius: 4px;
            display: inline-block;
            margin-bottom: 1rem;
            font-weight: bold;
        }}
        .album-header {{
            margin-bottom: 2rem;
            padding-bottom: 2rem;
            border-bottom: 2px solid #eee;
            display: flex;
            gap: 2rem;
            align-items: flex-start;
        }}
        .album-info {{
            flex: 1;
        }}
        .cover-art {{
            width: 300px;
            height: 300px;
            object-fit: cover;
            border-radius: 4px;
            box-shadow: 0 4px 12px rgba(0,0,0,0.15);
        }}
        h1 {{
            font-size: 2rem;
            margin-bottom: 0.5rem;
            color: #222;
        }}
        .artist {{
            font-size: 1.2rem;
            color: #666;
            margin-bottom: 0.5rem;
        }}
        .release-date {{
            color: #999;
            font-size: 0.9rem;
        }}
        .summary {{
            margin: 1rem 0;
            padding: 1rem;
            background: #f9f9f9;
            border-left: 3px solid #ff6b35;
        }}
        .tracks {{
            margin-top: 2rem;
        }}
        .tracks h2 {{
            font-size: 1.3rem;
            margin-bottom: 1rem;
            color: #222;
        }}
        .track {{
            display: grid;
            grid-template-columns: 2rem 1fr 4rem;
            gap: 1rem;
            align-items: center;
            padding: 1rem;
            border-bottom: 1px solid #eee;
        }}
        .track:last-child {{
            border-bottom: none;
        }}
        .track-number {{
            color: #999;
            font-weight: bold;
        }}
        .track-title {{
            font-weight: 500;
        }}
        .track-duration {{
            color: #999;
            font-size: 0.9rem;
            text-align: right;
        }}
        audio {{
            grid-column: 2 / -1;
            width: 100%;
            margin-top: 0.5rem;
        }}
        .footer {{
            margin-top: 2rem;
            padding-top: 2rem;
            border-top: 2px solid #eee;
            color: #999;
            font-size: 0.9rem;
            text-align: center;
        }}
        @media (max-width: 768px) {{
            .album-header {{
                flex-direction: column;
            }}
            .cover-art {{
                width: 100%;
                height: auto;
                max-width: 300px;
            }}
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="preview-badge">🚀 PREVIEW MODE - Live Reload Active</div>

        <div class="album-header">
            {}
            <div class="album-info">
                <h1>{}</h1>
                <div class="artist">by {}</div>
                <div class="release-date">Release: {}</div>
                <div class="summary">{}</div>
            </div>
        </div>

        <div class="tracks">
            <h2>Tracks</h2>
            {}
        </div>

        <div class="footer">
            Generated by release-kit • Press Ctrl+C to stop preview
        </div>
    </div>

    <script>
        // Hot reload via Server-Sent Events
        const eventSource = new EventSource('/_reload');
        eventSource.onmessage = () => {{
            console.log('Reloading...');
            location.reload();
        }};
        eventSource.onerror = () => {{
            console.log('Preview server disconnected');
            eventSource.close();
        }};

        // Pause other tracks when one starts playing
        document.addEventListener('DOMContentLoaded', () => {{
            const audioElements = document.querySelectorAll('audio');
            audioElements.forEach(audio => {{
                audio.addEventListener('play', () => {{
                    audioElements.forEach(other => {{
                        if (other !== audio) {{
                            other.pause();
                        }}
                    }});
                }});
            }});
        }});
    </script>
</body>
</html>"#,
        album.metadata.title,
        album.metadata.artist,
        cover_art_html,
        album.metadata.title,
        album.metadata.artist,
        album.metadata.release_date,
        album.metadata.summary,
        tracks_html
    );

    Html(html).into_response()
}

/// Format duration for display
fn format_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}
