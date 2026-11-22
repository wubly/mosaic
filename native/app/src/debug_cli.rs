use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use mosaic_core::model::TrackId;
use mosaic_core::storage::{load_config, Dirs};
use mosaic_services::lyrics::service::LyricsArgs;
use mosaic_services::Services;

fn services(dirs: Dirs) -> Result<Services> {
    let config = Arc::new(Mutex::new(load_config(&dirs)));
    Services::new(dirs, config)
}

pub fn run(dirs: Dirs, cmd: &[String]) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?;
    runtime.block_on(dispatch(dirs, cmd))
}

async fn dispatch(dirs: Dirs, cmd: &[String]) -> Result<()> {
    let name = cmd.first().map(String::as_str).context("missing command")?;
    let arg = |i: usize| {
        cmd.get(i)
            .map(String::as_str)
            .with_context(|| format!("`{name}` needs argument {i}"))
    };
    let services = services(dirs)?;

    match name {
        "sc-search" => {
            for t in services.sc_search(arg(1)?, 30).await? {
                println!(
                    "{}\t{} - {}\t{}ms",
                    t.id.key(),
                    t.artist,
                    t.title,
                    t.duration
                );
            }
        }
        "sc-stream" => {
            let id = TrackId::Num(arg(1)?.parse()?);
            let s = services.sc_stream(&id).await?;
            println!("{:?}\n{}\nsilence: {:?}", s.kind, s.url, s.silence);
        }
        "lyrics" => {
            let args = LyricsArgs {
                id: TrackId::Str(format!("debug:{}:{}", arg(1)?, arg(2)?)),
                title: arg(1)?.into(),
                artist: arg(2)?.into(),
                duration_ms: arg(3)?.parse()?,
                lrc_path: None,
            };
            let l = services.get_lyrics(&args).await;
            println!("{:?}", l.meta);
            for line in l.synced.iter().take(10) {
                println!("[{:.2}] {}", line.t, line.text);
            }
            if l.synced.is_empty() {
                println!(
                    "{}",
                    l.plain.lines().take(10).collect::<Vec<_>>().join("\n")
                );
            }
        }
        "scan" => {
            let tracks = services.scan_paths(&[arg(1)?.to_string()]).await?;
            for t in &tracks {
                println!(
                    "{}\t{} - {} [{}]\t{}ms\t{}",
                    t.id.key(),
                    t.artist,
                    t.title,
                    t.album,
                    t.duration,
                    t.artwork
                );
            }
            println!("{} tracks", tracks.len());
        }
        "play" => play_file(arg(1)?)?,
        other => bail!("unknown debug command `{other}`"),
    }
    Ok(())
}

fn play_file(path: &str) -> Result<()> {
    use mosaic_audio::{Command, DeckId, Engine, Event, SourceSpec};

    let engine = Engine::start()?;
    engine.send(Command::Load {
        deck: DeckId::A,
        gen: 1,
        source: SourceSpec::File(path.into()),
        start_at: 0.0,
        autoplay: true,
        fade_in_ms: 0,
        duration_hint: None,
    });
    let events = engine.events();
    let mut last_print = -1.0;
    for event in events.iter() {
        match event {
            Event::Position { secs, .. } if secs - last_print >= 1.0 => {
                println!("position {secs:.1}s");
                last_print = secs;
            }
            Event::Ended { .. } => break,
            Event::Error { message, .. } => bail!(message),
            Event::Ready { duration, .. } => println!("ready, duration {duration:?}"),
            _ => {}
        }
    }
    Ok(())
}
