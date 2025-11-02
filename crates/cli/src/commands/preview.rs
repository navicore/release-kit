use anyhow::{Context, Result};
use axum::{
    Router,
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
};
use notify::{Event as NotifyEvent, EventKind, RecursiveMode, Watcher};
use release_kit_core::config::parse_album_toml;
use std::{net::SocketAddr, path::PathBuf};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tower_http::services::ServeDir;

use super::build::build_static_site;

#[derive(Clone)]
struct AppState {
    reload_tx: broadcast::Sender<()>,
}

/// Start preview server with hot reload for local development.
///
/// This command:
/// - Builds the static site to a temporary directory
/// - Serves the built static files (exactly what will be deployed)
/// - Watches for file changes, rebuilds, and triggers hot reload
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
    println!();

    // Create temporary build directory (auto-cleanup on drop)
    let _temp_dir = TempDir::new().context("Failed to create temporary directory")?;
    let build_dir = _temp_dir.path();
    println!("📦 Building static site to temp directory...");
    build_static_site(&path, build_dir, false, None)
        .context("Failed to build static site for preview")?;
    println!("   ✓ Built to: {}", build_dir.display());

    // Create broadcast channel for reload events
    let (reload_tx, _) = broadcast::channel::<()>(100);

    let state = AppState {
        reload_tx: reload_tx.clone(),
    };

    // Build router - serve built static files
    let app = Router::new()
        .route("/_reload", get(sse_handler))
        .fallback_service(ServeDir::new(build_dir))
        .with_state(state);

    // Start file watcher with rebuild on change
    let watcher_source = path.clone();
    let watcher_build = build_dir.to_path_buf();
    let watcher_tx = reload_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = watch_and_rebuild(watcher_source, watcher_build, watcher_tx).await {
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

    // Set up graceful shutdown with Ctrl+C
    let server = axum::serve(listener, app).with_graceful_shutdown(async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");
        println!("\n🛑 Shutting down preview server...");
    });

    server.await.context("Server error")?;

    // TempDir cleanup happens automatically here when _temp_dir is dropped
    Ok(())
}

/// Watch for file changes, rebuild, and trigger reload
async fn watch_and_rebuild(
    source_path: PathBuf,
    build_path: PathBuf,
    reload_tx: broadcast::Sender<()>,
) -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    let mut watcher =
        notify::recommended_watcher(move |res: Result<NotifyEvent, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.blocking_send(event);
            }
        })?;

    // Watch album directory recursively
    watcher.watch(&source_path, RecursiveMode::Recursive)?;

    while let Some(event) = rx.recv().await {
        match event.kind {
            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                // Filter out temporary files and hidden files
                if event.paths.iter().any(|p| {
                    let filename = p.file_name().unwrap_or_default().to_string_lossy();
                    !filename.starts_with('.') && !filename.ends_with('~')
                }) {
                    println!("   📝 File changed, rebuilding...");

                    // Rebuild the static site
                    if let Err(e) = build_static_site(&source_path, &build_path, false, None) {
                        eprintln!("   ❌ Build failed: {}", e);
                    } else {
                        println!("   ✓ Rebuilt, reloading browser...");
                        let _ = reload_tx.send(());
                    }
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
