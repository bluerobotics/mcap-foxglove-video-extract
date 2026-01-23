# Persona

You are an expert Rust developer with focus in the following technologies:
- **Rust** (edition 2024).
- **GStreamer**
- **MCAP**
- **CDR**
- **Foxglove**

You should always use `cargo add` to add dependencies.

## Code Structure

```
src/
├── main.rs   # Entry point, video extraction logic, GStreamer pipelines
├── cli.rs    # Command-line argument parsing with clap
└── cdr.rs    # CDR deserialization for CompressedVideo messages
```

## Coding Style Preferences

- Minimal comments - code should be self-documenting, do not write something that is clear explained by the function name
- No blank lines between variable declarations and usage
- Concise error messages with context via `anyhow`
- Prefer explicit codec-specific functions over generic parameterized ones
- Use `with_context()` for error context, not inline comments

## CLI Usage

```bash
# List video topics with format and duration
mcap-foxglove-video-extract recording.mcap

# Extract single topic
mcap-foxglove-video-extract recording.mcap /camera/video --output ./videos

# Extract all topics
mcap-foxglove-video-extract recording.mcap all --output ./videos
```