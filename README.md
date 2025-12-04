# mcap-foxglove-video-extract

Rust CLI that lists `foxglove.CompressedVideo` topics in an MCAP file or extracts them to MP4 using GStreamer.

## Usage

```sh
mcap-foxglove-video-extract <mcap_file> [topic] [--output <output_dir>]
```

- No `topic` argument: list topics and durations.
- `topic=all`: extract every `foxglove.CompressedVideo` topic to MP4.
- Specific `topic`: extract only that topic.

Examples:

```sh
mcap-foxglove-video-extract potato.mcap
mcap-foxglove-video-extract potato.mcap video/UDPStream0/stream --output ./out
```
