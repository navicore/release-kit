use release_kit_core::types::Album;
use std::path::Path;

/// HTML-escape a string to prevent XSS attacks
///
/// Escapes: & < > " '
fn html_escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#x27;".to_string(),
            _ => c.to_string(),
        })
        .collect()
}

/// Detect cover art in artwork directory
pub fn detect_cover_art(artwork_dir: &Path) -> Option<String> {
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

/// Format duration for display
pub fn format_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Generate the complete HTML for the album player page
///
/// This template is shared between preview and build commands to ensure
/// what you see in preview is exactly what gets deployed.
///
/// # Arguments
///
/// * `album` - Album configuration
/// * `cover_art` - Optional cover art filename
/// * `is_preview` - Whether this is for preview mode (adds SSE reload)
/// * `audio_base_url` - Optional CDN base URL for audio files (e.g., "https://cdn.example.com")
pub fn generate_html(
    album: &Album,
    cover_art: Option<&str>,
    is_preview: bool,
    audio_base_url: Option<&str>,
) -> String {
    // Generate track list HTML with data attributes for player
    let tracks_html: String = album
        .tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let duration = track
                .duration
                .map(format_duration)
                .unwrap_or_else(|| String::from("--:--"));

            let filename = track
                .file
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("");

            // HTML-escape all user-provided strings to prevent XSS
            let escaped_filename = html_escape(filename);
            let escaped_title = html_escape(&track.title);

            // Construct audio URL: use CDN if provided, otherwise local /audio/
            let audio_url = if let Some(base_url) = audio_base_url {
                format!("{}/audio/{}", base_url, escaped_filename)
            } else {
                format!("/audio/{}", escaped_filename)
            };

            format!(
                r#"<div class="track" data-index="{}" data-src="{}" data-title="{}">
                    <span class="track-number">{:02}</span>
                    <span class="track-title">{}</span>
                    <span class="track-duration">{}</span>
                </div>"#,
                i,
                audio_url,
                escaped_title,
                i + 1,
                escaped_title,
                duration
            )
        })
        .collect();

    // Generate cover art HTML if it exists (with HTML escaping)
    let cover_art_html = if let Some(cover_filename) = cover_art {
        let escaped_cover = html_escape(cover_filename);
        format!(
            r#"<img src="/artwork/{}" alt="Album cover" class="cover-art">"#,
            escaped_cover
        )
    } else {
        String::new()
    };

    // Generate player album art HTML (smaller version, with HTML escaping)
    let player_art_html = if let Some(cover_filename) = cover_art {
        let escaped_cover = html_escape(cover_filename);
        format!(
            r#"<img src="/artwork/{}" alt="Album cover" class="player-album-art">"#,
            escaped_cover
        )
    } else {
        String::new()
    };

    // Preview badge only shown in preview mode
    let preview_badge = if is_preview {
        r#"<div class="preview-badge">🚀 PREVIEW MODE - Live Reload Active</div>"#
    } else {
        ""
    };

    // Hot reload script only in preview mode
    let reload_script = if is_preview {
        r#"<script>
        // Hot reload via Server-Sent Events
        const eventSource = new EventSource('/_reload');
        eventSource.onmessage = () => {
            console.log('Reloading...');
            location.reload();
        };
        eventSource.onerror = () => {
            console.log('Preview server disconnected');
            eventSource.close();
        };
    </script>"#
    } else {
        ""
    };

    // Footer text differs between preview and build
    let footer_text = if is_preview {
        "Generated by release-kit • Press Ctrl+C to stop preview"
    } else {
        "Generated by release-kit"
    };

    // HTML-escape all album metadata to prevent XSS
    let escaped_title = html_escape(&album.metadata.title);
    let escaped_artist = html_escape(&album.metadata.artist);
    let escaped_summary = html_escape(&album.metadata.summary);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{} - {}</title>
    <style>
        /* Theme - Metallic Analog Lab */
        :root {{
            --primary: #00ff88;
            --primary-focus: #00cc66;
            --base-100: #1a1a1f;
            --base-200: #222228;
            --base-300: #2a2a30;
            --base-content: #e0e0e0;
            --secondary: #4a4a5e;
            --neutral: #2a2a3e;
        }}

        * {{ margin: 0; padding: 0; box-sizing: border-box; }}

        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            line-height: 1.6;
            color: var(--base-content);
            background-color: var(--base-100);
            background-image:
                repeating-linear-gradient(
                    45deg,
                    transparent,
                    transparent 35px,
                    rgba(255, 255, 255, 0.01) 35px,
                    rgba(255, 255, 255, 0.01) 70px
                );
            padding: 2rem;
            padding-bottom: 200px; /* Space for player */
        }}

        .container {{
            max-width: 900px;
            margin: 0 auto;
            background: linear-gradient(135deg, var(--base-200) 0%, var(--base-100) 100%);
            padding: 2rem;
            border-radius: 8px;
            border: 1px solid rgba(255, 255, 255, 0.05);
            box-shadow:
                0 4px 6px rgba(0, 0, 0, 0.3),
                inset 0 1px 0 rgba(255, 255, 255, 0.05);
        }}

        .preview-badge {{
            background: linear-gradient(135deg, var(--primary-focus) 0%, #00aa55 100%);
            color: #000000;
            padding: 0.5rem 1rem;
            border-radius: 4px;
            display: inline-block;
            margin-bottom: 1.5rem;
            font-weight: bold;
            text-shadow: 0 1px 0 rgba(255, 255, 255, 0.2);
            box-shadow: 0 2px 8px rgba(0, 255, 136, 0.3);
        }}

        .album-header {{
            margin-bottom: 2rem;
            padding-bottom: 2rem;
            border-bottom: 2px solid rgba(255, 255, 255, 0.1);
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
            box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
            border: 1px solid rgba(255, 255, 255, 0.1);
        }}

        h1 {{
            font-size: 2.5rem;
            margin-bottom: 0.5rem;
            color: var(--primary);
            text-shadow: 0 0 20px var(--primary);
        }}

        .artist {{
            font-size: 1.4rem;
            color: var(--base-content);
            margin-bottom: 0.5rem;
            opacity: 0.9;
        }}

        .release-date {{
            color: var(--base-content);
            opacity: 0.6;
            font-size: 0.9rem;
        }}

        .summary {{
            margin: 1rem 0;
            padding: 1rem;
            background: rgba(0, 0, 0, 0.3);
            border-left: 3px solid var(--primary);
            border-radius: 4px;
            line-height: 1.8;
        }}

        .tracks {{
            margin-top: 2rem;
        }}

        .tracks h2 {{
            font-size: 1.3rem;
            margin-bottom: 1rem;
            color: var(--primary);
            text-shadow: 0 0 10px var(--primary);
        }}

        .track {{
            display: grid;
            grid-template-columns: 3rem 1fr 5rem;
            gap: 1rem;
            align-items: center;
            padding: 1rem;
            border-bottom: 1px solid rgba(255, 255, 255, 0.05);
            cursor: pointer;
            transition: all 0.2s ease;
            border-radius: 4px;
        }}

        .track:hover {{
            background-color: rgba(0, 255, 136, 0.05);
            transform: translateX(4px);
        }}

        .track.playing {{
            background: linear-gradient(90deg, rgba(0, 255, 136, 0.1) 0%, transparent 100%);
            border-left: 3px solid var(--primary);
        }}

        .track-number {{
            color: var(--base-content);
            opacity: 0.5;
            font-weight: bold;
            font-size: 0.9rem;
        }}

        .track.playing .track-number {{
            color: var(--primary);
            opacity: 1;
        }}

        .track-title {{
            font-weight: 500;
            color: var(--base-content);
        }}

        .track.playing .track-title {{
            color: var(--primary);
        }}

        .track-duration {{
            color: var(--base-content);
            opacity: 0.5;
            font-size: 0.9rem;
            text-align: right;
        }}

        /* Fixed Player at Bottom */
        .player {{
            position: fixed;
            bottom: 0;
            left: 0;
            right: 0;
            background: linear-gradient(135deg, var(--base-200) 0%, var(--base-100) 100%);
            border-top: 2px solid rgba(0, 255, 136, 0.2);
            padding: 1rem;
            box-shadow:
                0 -4px 20px rgba(0, 0, 0, 0.5),
                inset 0 1px 0 rgba(255, 255, 255, 0.05);
            z-index: 1000;
        }}

        .player-content {{
            max-width: 1200px;
            margin: 0 auto;
            display: grid;
            grid-template-columns: auto 1fr;
            grid-template-rows: auto auto;
            gap: 0.5rem 1.5rem;
            align-items: center;
        }}

        .player-album-art {{
            grid-row: 1 / 3;
            width: 96px;
            height: 96px;
            object-fit: cover;
            border-radius: 4px;
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
            border: 1px solid rgba(255, 255, 255, 0.1);
        }}

        .player-right {{
            grid-column: 2;
            display: flex;
            flex-direction: column;
            gap: 0.5rem;
        }}

        .player-info-controls {{
            display: flex;
            align-items: center;
            gap: 2rem;
        }}

        .player-info {{
            display: flex;
            flex-direction: column;
            gap: 0.25rem;
            flex: 1;
        }}

        .player-track {{
            font-size: 1rem;
            font-weight: 600;
            color: var(--primary);
        }}

        .player-artist {{
            font-size: 0.85rem;
            color: var(--base-content);
            opacity: 0.7;
        }}

        .player-controls {{
            display: flex;
            gap: 0.75rem;
            align-items: center;
        }}

        .player-btn {{
            background: linear-gradient(135deg, var(--secondary) 0%, var(--neutral) 100%);
            border: 1px solid rgba(255, 255, 255, 0.1);
            color: var(--base-content);
            width: 40px;
            height: 40px;
            border-radius: 50%;
            cursor: pointer;
            display: flex;
            align-items: center;
            justify-content: center;
            transition: all 0.2s ease;
            box-shadow:
                0 2px 4px rgba(0, 0, 0, 0.3),
                inset 0 1px 0 rgba(255, 255, 255, 0.05);
        }}

        .player-btn:hover {{
            box-shadow:
                0 4px 8px rgba(0, 255, 136, 0.2),
                0 2px 4px rgba(0, 0, 0, 0.4);
            transform: translateY(-2px);
        }}

        .player-btn:active {{
            transform: translateY(0);
            box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.4);
        }}

        .player-btn.play {{
            width: 50px;
            height: 50px;
            background: linear-gradient(135deg, var(--primary-focus) 0%, #00aa55 100%);
            color: #000000;
        }}

        .player-progress {{
            grid-column: 1 / -1;
            margin-top: 0.5rem;
        }}

        .progress-bar {{
            width: 100%;
            height: 6px;
            background: rgba(0, 0, 0, 0.3);
            border-radius: 3px;
            cursor: pointer;
            position: relative;
            overflow: hidden;
            border: 1px solid var(--secondary);
        }}

        .progress-fill {{
            height: 100%;
            background: linear-gradient(90deg, var(--primary) 0%, var(--primary-focus) 100%);
            width: 0%;
            transition: width 0.1s linear;
            box-shadow: 0 0 10px var(--primary);
        }}

        .oscilloscope {{
            width: 100%;
            max-width: 600px;
            height: 70px;
            background: rgba(0, 8, 5, 1);
            border-radius: 4px;
            border: 1px solid rgba(255, 255, 255, 0.1);
            box-shadow:
                inset 0 0 20px rgba(0, 255, 136, 0.1),
                0 2px 4px rgba(0, 0, 0, 0.3);
        }}

        .footer {{
            margin-top: 2rem;
            padding-top: 2rem;
            border-top: 2px solid rgba(255, 255, 255, 0.1);
            color: var(--base-content);
            opacity: 0.5;
            font-size: 0.9rem;
            text-align: center;
        }}

        audio {{
            display: none;
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
            .player-content {{
                grid-template-columns: auto 1fr;
                grid-template-rows: auto;
                gap: 0.75rem;
            }}
            .player-album-art {{
                grid-row: 1;
                width: 50px;
                height: 50px;
            }}
            .oscilloscope {{
                display: none;
            }}
            .player-info-controls {{
                flex-direction: column;
                align-items: flex-start;
                gap: 0.75rem;
            }}
            .player-track {{
                font-size: 0.9rem;
            }}
            .player-artist {{
                font-size: 0.75rem;
            }}
        }}
    </style>
</head>
<body>
    <div class="container">
        {}

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
            <div id="track-list">
                {}
            </div>
        </div>

        <div class="footer">
            {}
        </div>
    </div>

    <!-- Fixed Player -->
    <div class="player">
        <div class="player-content">
            {}
            <div class="player-right">
                <canvas id="oscilloscope" class="oscilloscope" width="1200" height="140"></canvas>
                <div class="player-info-controls">
                    <div class="player-info">
                        <div class="player-track" id="player-track">Select a track</div>
                        <div class="player-artist" id="player-artist">{}</div>
                    </div>
                    <div class="player-controls">
                        <button class="player-btn" id="prev-btn">
                            <svg width="20" height="20" fill="currentColor" viewBox="0 0 20 20">
                                <path d="M14 4v12M12 6l-6 6 6 6V6z"/>
                            </svg>
                        </button>
                        <button class="player-btn play" id="play-btn">
                            <svg id="play-icon" width="24" height="24" fill="currentColor">
                                <path d="M8 5v14l11-7z"/>
                            </svg>
                            <svg id="pause-icon" width="24" height="24" fill="currentColor" style="display:none">
                                <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z"/>
                            </svg>
                        </button>
                        <button class="player-btn" id="next-btn">
                            <svg width="20" height="20" fill="currentColor">
                                <path d="M18 4v12M16 6l-6 6 6 6V6z" transform="scale(-1, 1) translate(-24, 0)"/>
                            </svg>
                        </button>
                    </div>
                </div>
            </div>
        </div>

        <div class="player-progress">
            <div class="progress-bar" id="progress-bar">
                <div class="progress-fill" id="progress-fill"></div>
            </div>
        </div>
    </div>

    <audio id="audio" preload="metadata"></audio>

    {}
    <script src="{}"></script>
</body>
</html>"#,
        escaped_title,
        escaped_artist,
        preview_badge,
        cover_art_html,
        escaped_title,
        escaped_artist,
        album.metadata.release_date,
        escaped_summary,
        tracks_html,
        footer_text,
        player_art_html,
        escaped_artist,
        reload_script,
        if is_preview {
            "/_player.js"
        } else {
            "/player.js"
        }
    )
}

/// Generate the player JavaScript code
///
/// This is the same for both preview and build modes.
pub fn generate_player_js() -> &'static str {
    r#"// Audio Player with Oscilloscope Visualization
class AnalogOscilloscope {
    constructor(canvas, analyser) {
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
        this.analyser = analyser;
        this.dataArray = new Uint8Array(analyser.frequencyBinCount);
        this.animationId = null;
        this.isRunning = false;
    }

    start() {
        if (this.isRunning) return;
        this.isRunning = true;
        this.draw();
    }

    stop() {
        this.isRunning = false;
        if (this.animationId) {
            cancelAnimationFrame(this.animationId);
            this.animationId = null;
        }
        this.ctx.fillStyle = 'rgba(0, 8, 5, 1)';
        this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
    }

    draw() {
        if (!this.isRunning) return;

        this.animationId = requestAnimationFrame(() => this.draw());
        this.analyser.getByteTimeDomainData(this.dataArray);

        this.ctx.fillStyle = 'rgba(0, 8, 5, 0.1)';
        this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);

        this.drawGrid();

        this.ctx.lineWidth = 2;
        this.ctx.strokeStyle = '#00ff88';
        this.ctx.shadowBlur = 15;
        this.ctx.shadowColor = '#00ff88';
        this.ctx.beginPath();

        const sliceWidth = this.canvas.width / this.dataArray.length;
        let x = 0;

        for (let i = 0; i < this.dataArray.length; i++) {
            const v = this.dataArray[i] / 128.0;
            const y = (v * this.canvas.height) / 2;

            if (i === 0) {
                this.ctx.moveTo(x, y);
            } else {
                this.ctx.lineTo(x, y);
            }

            x += sliceWidth;
        }

        this.ctx.stroke();
        this.ctx.shadowBlur = 0;
    }

    drawGrid() {
        this.ctx.strokeStyle = 'rgba(0, 255, 136, 0.1)';
        this.ctx.lineWidth = 1;

        const numHLines = 4;
        for (let i = 0; i <= numHLines; i++) {
            const y = (i * this.canvas.height) / numHLines;
            this.ctx.beginPath();
            this.ctx.moveTo(0, y);
            this.ctx.lineTo(this.canvas.width, y);
            this.ctx.stroke();
        }

        const numVLines = 10;
        for (let i = 0; i <= numVLines; i++) {
            const x = (i * this.canvas.width) / numVLines;
            this.ctx.beginPath();
            this.ctx.moveTo(x, 0);
            this.ctx.lineTo(x, this.canvas.height);
            this.ctx.stroke();
        }
    }
}

class AudioPlayer {
    constructor() {
        this.audio = document.getElementById('audio');
        this.tracks = Array.from(document.querySelectorAll('.track'));
        this.currentTrackIndex = -1;

        this.playBtn = document.getElementById('play-btn');
        this.prevBtn = document.getElementById('prev-btn');
        this.nextBtn = document.getElementById('next-btn');
        this.playIcon = document.getElementById('play-icon');
        this.pauseIcon = document.getElementById('pause-icon');
        this.progressBar = document.getElementById('progress-bar');
        this.progressFill = document.getElementById('progress-fill');
        this.playerTrackEl = document.getElementById('player-track');
        this.playerArtistEl = document.getElementById('player-artist');

        this.audioContext = null;
        this.analyser = null;
        this.source = null;
        this.oscilloscope = null;

        this.initializeAudio();
        this.attachEventListeners();
        this.initializeOscilloscope();
    }

    initializeAudio() {
        this.audio.addEventListener('timeupdate', () => this.updateProgress());
        this.audio.addEventListener('ended', () => this.next());
        this.audio.addEventListener('play', () => this.updatePlayButton(true));
        this.audio.addEventListener('pause', () => this.updatePlayButton(false));
    }

    initializeOscilloscope() {
        const canvas = document.getElementById('oscilloscope');
        if (!canvas) return;

        const setupAudioContext = () => {
            if (this.audioContext) {
                // Resume if suspended (autoplay policy)
                if (this.audioContext.state === 'suspended') {
                    this.audioContext.resume();
                }
                return;
            }

            this.audioContext = new (window.AudioContext || window.webkitAudioContext)();
            this.analyser = this.audioContext.createAnalyser();
            this.analyser.fftSize = 2048;

            // Only create source if it doesn't exist (can only call once per audio element)
            if (!this.source) {
                this.source = this.audioContext.createMediaElementSource(this.audio);
                this.source.connect(this.analyser);
            }
            this.analyser.connect(this.audioContext.destination);

            this.oscilloscope = new AnalogOscilloscope(canvas, this.analyser);
        };

        this.audio.addEventListener('play', () => {
            setupAudioContext();
            if (this.oscilloscope) {
                this.oscilloscope.start();
            }
        });

        this.audio.addEventListener('pause', () => {
            if (this.oscilloscope) {
                this.oscilloscope.stop();
            }
        });
    }

    attachEventListeners() {
        this.tracks.forEach((track, index) => {
            track.addEventListener('click', () => this.playTrack(index));
        });

        this.playBtn.addEventListener('click', () => this.togglePlay());
        this.prevBtn.addEventListener('click', () => this.previous());
        this.nextBtn.addEventListener('click', () => this.next());

        this.progressBar.addEventListener('click', (e) => this.seek(e));

        document.addEventListener('keydown', (e) => {
            if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;

            if (e.code === 'Space') {
                e.preventDefault();
                this.togglePlay();
            } else if (e.code === 'ArrowLeft') {
                e.preventDefault();
                this.previous();
            } else if (e.code === 'ArrowRight') {
                e.preventDefault();
                this.next();
            }
        });
    }

    playTrack(index) {
        if (index < 0 || index >= this.tracks.length) return;

        const track = this.tracks[index];
        const src = track.dataset.src;
        const title = track.dataset.title;

        this.currentTrackIndex = index;

        this.tracks.forEach(t => t.classList.remove('playing'));
        track.classList.add('playing');

        this.playerTrackEl.textContent = title;

        this.audio.src = src;
        this.audio.play().catch(err => {
            console.error('Playback failed:', err);
            this.showError(`Failed to play "${title}": ${err.message}`);
            track.classList.remove('playing');
        });
    }

    showError(message) {
        // Display error to user
        this.playerTrackEl.textContent = '⚠️ ' + message;
        this.playerTrackEl.style.color = '#ff006e';
        setTimeout(() => {
            this.playerTrackEl.style.color = '';
            if (this.currentTrackIndex >= 0) {
                this.playerTrackEl.textContent = this.tracks[this.currentTrackIndex]?.dataset.title || 'Select a track';
            } else {
                this.playerTrackEl.textContent = 'Select a track';
            }
        }, 3000);
    }

    togglePlay() {
        if (this.currentTrackIndex === -1 && this.tracks.length > 0) {
            this.playTrack(0);
        } else if (this.audio.paused) {
            this.audio.play();
        } else {
            this.audio.pause();
        }
    }

    previous() {
        if (this.currentTrackIndex > 0) {
            this.playTrack(this.currentTrackIndex - 1);
        }
    }

    next() {
        if (this.currentTrackIndex < this.tracks.length - 1) {
            this.playTrack(this.currentTrackIndex + 1);
        }
    }

    seek(e) {
        const rect = this.progressBar.getBoundingClientRect();
        const percent = (e.clientX - rect.left) / rect.width;
        this.audio.currentTime = percent * this.audio.duration;
    }

    updateProgress() {
        if (!this.audio.duration) return;
        const percent = (this.audio.currentTime / this.audio.duration) * 100;
        this.progressFill.style.width = `${percent}%`;
    }

    updatePlayButton(isPlaying) {
        if (isPlaying) {
            this.playIcon.style.display = 'none';
            this.pauseIcon.style.display = 'block';
        } else {
            this.playIcon.style.display = 'block';
            this.pauseIcon.style.display = 'none';
        }
    }
}

if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
        window.audioPlayer = new AudioPlayer();
    });
} else {
    window.audioPlayer = new AudioPlayer();
}
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape_basic_characters() {
        assert_eq!(html_escape("Hello World"), "Hello World");
        assert_eq!(html_escape("Test & Test"), "Test &amp; Test");
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(html_escape("'single'"), "&#x27;single&#x27;");
    }

    #[test]
    fn test_html_escape_xss_attempts() {
        // Test common XSS attack vectors
        assert_eq!(
            html_escape("<script>alert('XSS')</script>"),
            "&lt;script&gt;alert(&#x27;XSS&#x27;)&lt;/script&gt;"
        );

        assert_eq!(
            html_escape("\"><script>alert(document.cookie)</script>"),
            "&quot;&gt;&lt;script&gt;alert(document.cookie)&lt;/script&gt;"
        );

        assert_eq!(
            html_escape("' onload='alert(1)"),
            "&#x27; onload=&#x27;alert(1)"
        );

        assert_eq!(
            html_escape("<img src=x onerror=alert(1)>"),
            "&lt;img src=x onerror=alert(1)&gt;"
        );
    }

    #[test]
    fn test_html_escape_combined_characters() {
        assert_eq!(
            html_escape("A&B<C>D\"E'F"),
            "A&amp;B&lt;C&gt;D&quot;E&#x27;F"
        );
    }

    #[test]
    fn test_html_escape_empty_string() {
        assert_eq!(html_escape(""), "");
    }

    #[test]
    fn test_html_escape_unicode() {
        // Unicode should pass through unchanged
        assert_eq!(html_escape("トラック01"), "トラック01");
        assert_eq!(html_escape("piste numéro 1"), "piste numéro 1");
        assert_eq!(html_escape("трек 01"), "трек 01");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(std::time::Duration::from_secs(0)), "0:00");
        assert_eq!(format_duration(std::time::Duration::from_secs(59)), "0:59");
        assert_eq!(format_duration(std::time::Duration::from_secs(60)), "1:00");
        assert_eq!(format_duration(std::time::Duration::from_secs(323)), "5:23");
        assert_eq!(
            format_duration(std::time::Duration::from_secs(3599)),
            "59:59"
        );
        assert_eq!(
            format_duration(std::time::Duration::from_secs(3661)),
            "61:01"
        );
    }
}
