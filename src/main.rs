mod cdr;
mod cli;

use std::{
    collections::{HashMap, HashSet},
    fmt,
    fs,
    path::Path,
};

use anyhow::{Context, Result};
use cdr::decode_compressed_video;
use clap::Parser;
use cli::Cli;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use mcap::MessageStream;
use memmap2::MmapOptions;

const MESSAGE_SCHEMA_NAME: &str = "foxglove.CompressedVideo";

/// Supported video codecs for extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFormat {
    H264,
    H265,
}

impl VideoFormat {
    /// Parse the video format from a Foxglove CompressedVideo format string.
    pub fn from_format_string(format: &str) -> Option<Self> {
        let normalized = format.to_lowercase();
        match normalized.as_str() {
            "h264" | "avc" | "h.264" => Some(VideoFormat::H264),
            "h265" | "hevc" | "h.265" => Some(VideoFormat::H265),
            _ => None,
        }
    }

    /// Returns the file extension for output files.
    pub fn file_extension(&self) -> &'static str {
        match self {
            VideoFormat::H264 => "mp4",
            VideoFormat::H265 => "mp4",
        }
    }
}

impl fmt::Display for VideoFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VideoFormat::H264 => write!(f, "H.264"),
            VideoFormat::H265 => write!(f, "H.265"),
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Cli::parse();
    let mapped = map_mcap(&args.mcap_file)?;

    match args.topic.as_deref() {
        None => list_video_topics(&mapped)?,
        Some("all") => {
            let topics = get_video_topics(&mapped)?;
            if topics.is_empty() {
                println!("No foxglove.CompressedVideo messages found");
                return Ok(());
            }
            fs::create_dir_all(&args.output).with_context(|| {
                format!("unable to create output dir {}", args.output.display())
            })?;
            for topic in topics {
                extract_video(&mapped, &topic, &args.output)?;
            }
        }
        Some(topic) => {
            fs::create_dir_all(&args.output).with_context(|| {
                format!("unable to create output dir {}", args.output.display())
            })?;
            extract_video(&mapped, topic, &args.output)?;
        }
    }

    Ok(())
}

fn map_mcap(path: &Path) -> Result<memmap2::Mmap> {
    let file = fs::File::open(path)
        .with_context(|| format!("unable to open MCAP file {}", path.display()))?;
    let mmap = unsafe { MmapOptions::new().map(&file) }
        .with_context(|| format!("unable to memory map {}", path.display()))?;
    Ok(mmap)
}

fn list_video_topics(mapped: &memmap2::Mmap) -> Result<()> {
    let info = get_topic_info(mapped)?;
    if info.is_empty() {
        println!("No foxglove.CompressedVideo messages found");
        return Ok(());
    }

    println!("\nFound foxglove.CompressedVideo messages on topics:");
    let mut topics: Vec<_> = info.keys().cloned().collect();
    topics.sort();
    for topic in topics {
        if let Some((duration, format)) = info.get(&topic) {
            let format_str = format
                .as_ref()
                .map(|f| f.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            println!("- {topic} ({format_str}, {duration}s)");
        }
    }

    Ok(())
}

fn get_video_topics(mapped: &memmap2::Mmap) -> Result<HashSet<String>> {
    let mut topics = HashSet::new();
    for msg in MessageStream::new(mapped)? {
        let msg = msg?;
        if is_video_message(&msg) {
            topics.insert(msg.channel.topic.clone());
        }
    }
    Ok(topics)
}

/// Information about a video topic: (duration_seconds, detected_format)
type TopicInfo = (u64, Option<VideoFormat>);

fn get_topic_info(mapped: &memmap2::Mmap) -> Result<HashMap<String, TopicInfo>> {
    let mut spans: HashMap<String, (Option<u64>, Option<u64>, Option<VideoFormat>)> =
        HashMap::new();

    for msg in MessageStream::new(mapped)? {
        let msg = msg?;
        if !is_video_message(&msg) {
            continue;
        }

        let Ok(video) = decode_compressed_video(msg.data.as_ref()) else {
            continue;
        };
        let ts = video.timestamp.as_nanos();
        let entry = spans
            .entry(msg.channel.topic.clone())
            .or_insert((None, None, None));

        if entry.0.is_none() {
            entry.0 = Some(ts);
            entry.2 = VideoFormat::from_format_string(&video.format);
        }
        entry.1 = Some(ts);
    }

    let info = spans
        .into_iter()
        .map(|(topic, (first, last, format))| {
            let duration = match (first, last) {
                (Some(start), Some(end)) if end >= start => (end - start) / 1_000_000_000,
                _ => 0,
            };
            (topic, (duration, format))
        })
        .collect();

    Ok(info)
}

fn detect_video_format(mapped: &memmap2::Mmap, topic: &str) -> Result<VideoFormat> {
    for msg in MessageStream::new(mapped)? {
        let msg = msg?;
        if !(is_video_message(&msg) && msg.channel.topic == topic) {
            continue;
        }

        let video = decode_compressed_video(msg.data.as_ref())
            .with_context(|| format!("failed to decode first video message on {topic}"))?;

        return VideoFormat::from_format_string(&video.format).ok_or_else(|| {
            anyhow::anyhow!(
                "unsupported video format '{}' on topic {topic}. Supported formats: h264, h265/hevc",
                video.format
            )
        });
    }

    Err(anyhow::anyhow!(
        "no video messages found on topic {topic}"
    ))
}

fn extract_video(mapped: &memmap2::Mmap, topic: &str, output_dir: &Path) -> Result<()> {
    let format = detect_video_format(mapped, topic)?;
    println!(
        "Extracting {format} video from topic {topic} to {}",
        output_dir.display()
    );
    gst::init()?;

    let safe_topic = topic.replace('/', "_");
    let output_file = output_dir.join(format!("{safe_topic}.{}", format.file_extension()));

    let (pipeline, appsrc) = build_pipeline(&output_file, format)?;
    let bus = pipeline.bus().context("pipeline missing bus")?;

    pipeline
        .set_state(gst::State::Playing)
        .context("failed to start pipeline")?;
    let _ = bus.timed_pop_filtered(
        gst::ClockTime::from_seconds(5),
        &[gst::MessageType::StateChanged],
    );

    let mut prev_publish: Option<u64> = None;
    let mut frame_count = 0usize;

    for msg in MessageStream::new(mapped)? {
        let msg = msg?;
        if !(is_video_message(&msg) && msg.channel.topic == topic) {
            continue;
        }

        let video = match decode_compressed_video(msg.data.as_ref()) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("Failed to decode CDR message on {topic}: {err}");
                continue;
            }
        };

        let mut buffer = gst::Buffer::from_slice(video.data);
        {
            let buffer = buffer.get_mut().context("buffer not writable")?;

            let duration_ns = prev_publish
                .map(|prev| msg.publish_time.saturating_sub(prev).max(1))
                .unwrap_or(1_000_000_000 / 30);
            let pts = gst::ClockTime::from_nseconds(msg.publish_time);
            buffer.set_pts(pts);
            buffer.set_dts(pts);
            buffer.set_duration(gst::ClockTime::from_nseconds(duration_ns));
        }

        match appsrc.push_buffer(buffer) {
            Ok(gst::FlowSuccess::Ok) => {
                frame_count += 1;
                prev_publish = Some(msg.publish_time);
            }
            Ok(other) => {
                eprintln!("Unexpected flow return when pushing buffer: {other:?}");
                break;
            }
            Err(err) => {
                eprintln!("Failed to push buffer: {err}");
                break;
            }
        }
    }

    appsrc.end_of_stream().context("failed to signal EOS")?;
    let msg = bus.timed_pop_filtered(
        gst::ClockTime::NONE, // We wait forever to allow low end devices to finish
        &[gst::MessageType::Eos, gst::MessageType::Error],
    );

    let res = match msg {
        Some(message) => match message.view() {
            gst::MessageView::Eos(_) => {
                println!(
                    "Successfully finished writing {} ({} frames)",
                    output_file.display(),
                    frame_count
                );
                Ok(())
            }
            gst::MessageView::Error(err) => {
                Err(anyhow::anyhow!("GStreamer error: {}", err.error()))
            }
            _ => Err(anyhow::anyhow!("No EOS message received before timeout")),
        },
        None => Err(anyhow::anyhow!("No EOS message received before timeout")),
    };

    pipeline
        .set_state(gst::State::Null)
        .context("failed to tear down pipeline")?;

    res
}

fn build_pipeline(
    output_path: &Path,
    format: VideoFormat,
) -> Result<(gst::Pipeline, gst_app::AppSrc)> {
    match format {
        VideoFormat::H264 => build_pipeline_h264(output_path),
        VideoFormat::H265 => build_pipeline_h265(output_path),
    }
}

fn build_pipeline_h264(output_path: &Path) -> Result<(gst::Pipeline, gst_app::AppSrc)> {
    let pipeline = gst::Pipeline::new();

    let caps = gst::Caps::builder("video/x-h264")
        .field("stream-format", "byte-stream")
        .field("framerate", gst::Fraction::new(30, 1))
        .build();

    let appsrc = gst_app::AppSrc::builder()
        .name("src")
        .caps(&caps)
        .is_live(false)
        .do_timestamp(true)
        .format(gst::Format::Time)
        .build();
    appsrc.set_do_timestamp(true);

    let h264parse = gst::ElementFactory::make("h264parse")
        .build()
        .context("missing h264parse element")?;
    let mp4mux = gst::ElementFactory::make("mp4mux")
        .property("faststart", true)
        .build()
        .context("missing mp4mux element")?;
    let filesink = gst::ElementFactory::make("filesink")
        .property("location", output_path.to_string_lossy().to_string())
        .build()
        .context("missing filesink element")?;

    pipeline.add_many([
        appsrc.upcast_ref::<gst::Element>(),
        h264parse.as_ref(),
        mp4mux.as_ref(),
        filesink.as_ref(),
    ])?;
    gst::Element::link_many([
        appsrc.upcast_ref::<gst::Element>(),
        h264parse.as_ref(),
        mp4mux.as_ref(),
        filesink.as_ref(),
    ])?;

    Ok((pipeline, appsrc))
}

fn build_pipeline_h265(output_path: &Path) -> Result<(gst::Pipeline, gst_app::AppSrc)> {
    let pipeline = gst::Pipeline::new();

    let caps = gst::Caps::builder("video/x-h265")
        .field("stream-format", "byte-stream")
        .field("framerate", gst::Fraction::new(30, 1))
        .build();

    let appsrc = gst_app::AppSrc::builder()
        .name("src")
        .caps(&caps)
        .is_live(false)
        .do_timestamp(true)
        .format(gst::Format::Time)
        .build();
    appsrc.set_do_timestamp(true);

    let h265parse = gst::ElementFactory::make("h265parse")
        .build()
        .context("missing h265parse element")?;
    let mp4mux = gst::ElementFactory::make("mp4mux")
        .property("faststart", true)
        .build()
        .context("missing mp4mux element")?;
    let filesink = gst::ElementFactory::make("filesink")
        .property("location", output_path.to_string_lossy().to_string())
        .build()
        .context("missing filesink element")?;

    pipeline.add_many([
        appsrc.upcast_ref::<gst::Element>(),
        h265parse.as_ref(),
        mp4mux.as_ref(),
        filesink.as_ref(),
    ])?;
    gst::Element::link_many([
        appsrc.upcast_ref::<gst::Element>(),
        h265parse.as_ref(),
        mp4mux.as_ref(),
        filesink.as_ref(),
    ])?;

    Ok((pipeline, appsrc))
}

fn is_video_message(msg: &mcap::Message<'_>) -> bool {
    msg.channel
        .schema
        .as_ref()
        .map(|schema| schema.name == MESSAGE_SCHEMA_NAME)
        .unwrap_or(false)
}
