use anyhow::Result;
use clap::Parser;

use bili_player_cli::bilibili::api::BilibiliClient;
use bili_player_cli::bilibili::search::extract_bvid;
use bili_player_cli::bilibili::stream::get_audio_stream;
use bili_player_cli::bilibili::video::get_video_info;
use bili_player_cli::cli::Commands;
use bili_player_cli::config::Config;
use bili_player_cli::player::mpv::MpvBackend;

#[tokio::main]
async fn main() -> Result<()> {
    let args = bili_player_cli::cli::Cli::parse();

    // Init logging
    if args.debug {
        tracing_subscriber::fmt().init();
    }

    // Load config
    let config = Config::load(args.config.as_deref())?;

    match args.command {
        Some(Commands::Play { input }) => play_command(&config, &input).await?,
        None => {
            // TUI mode
            let mut app = bili_player_cli::app::App::new(config)?;
            app.run().await?;
            std::process::exit(0);
        }
    }

    Ok(())
}

async fn play_command(config: &Config, input: &str) -> Result<()> {
    // Extract BV number
    let bvid = extract_bvid(input)
        .ok_or_else(|| anyhow::anyhow!("Invalid input: expected BV number or Bilibili URL"))?;

    println!("Resolving: {bvid}");

    // Create API client
    let client = BilibiliClient::new(Some(config.bilibili.sessdata.clone()));

    // Get video info (cid)
    let track = get_video_info(&client, &bvid).await?;
    println!("Title: {}", track.title);
    println!("Author: {}", track.author);
    println!("Duration: {}s", track.duration.as_secs());

    // Get audio stream URL
    let source = get_audio_stream(&client, &track.bvid, track.cid).await?;
    println!(
        "Audio quality: {}",
        match &source.audio_quality {
            bili_player_cli::queue::track::AudioQuality::Flac => "FLAC",
            bili_player_cli::queue::track::AudioQuality::DolbyAtmos => "Dolby Atmos",
            bili_player_cli::queue::track::AudioQuality::Dash { bitrate } => {
                match bitrate {
                    30280 => "Hi-Res (192K)",
                    30232 => "Medium (132K)",
                    30216 => "Standard (64K)",
                    _ => "DASH",
                }
            }
            bili_player_cli::queue::track::AudioQuality::LegacyMp4 => "Legacy MP4",
        }
    );

    // Create mpv backend and play
    let mut mpv = MpvBackend::new()?;
    mpv.set_volume(config.player.volume);
    mpv.play(&source)?;

    println!("Playing... (Ctrl+C to stop)");

    // Block until playback ends using the event receiver
    while let Some(event) = mpv.event_rx().recv().await {
        match event {
            bili_player_cli::event::PlayerEvent::TrackEnded { .. } => break,
            bili_player_cli::event::PlayerEvent::Shutdown => break,
            _ => {}
        }
    }

    let _ = mpv.shutdown();
    Ok(())
}
