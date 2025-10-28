# Test Album

This is an example album directory for testing release-kit.

Note: The audio files are placeholders and don't actually exist. In a real album, you would have:

```
audio/
├── 01-infrastructure-hum.flac
├── 02-resonant-decay.flac
└── 03-harmonic-collapse.flac
```

And artwork:

```
artwork/
├── cover.jpg (3000x3000 recommended)
└── banner.jpg (optional)
```

To test validation without audio files, use:

```bash
release-kit validate examples/test-album/
```

This will parse the album.toml but may warn about missing audio files.
