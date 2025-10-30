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

use super::template::{detect_cover_art, generate_html, generate_player_js};

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
        .route("/_player.js", get(player_js_handler))
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

/// Serve player JavaScript with cache headers
async fn player_js_handler() -> Response {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (axum::http::header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        generate_player_js(),
    )
        .into_response()
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

    // Generate HTML using shared template
    let html = generate_html(&album, cover_art.as_deref(), true);

    Html(html).into_response()
}
