use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use mosaic_core::model::TrackId;
use mosaic_core::storage::{load_config, Dirs};
use mosaic_services::lyrics::service::LyricsArgs;
use mosaic_services::soundcloud::client_types::StreamKind;
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
        "yt-stream" => {
            let p = services.yt_stream(arg(1)?).await?;
            println!(
                "{}\t{}kbps\texpires in {}s",
                p.mime,
                p.bitrate / 1000,
                p.expires_in
            );
            println!("{}", p.url);
        }
        "yt-video" => {
            let v = services.yt_video_stream(arg(1)?).await?;
            println!(
                "{}x{} @{}fps\texpires in {}s",
                v.width, v.height, v.fps, v.expires_in
            );
            println!("{}", v.url);
        }
        "yt-watch" => {
            let hits = services.yt_search(arg(1)?).await?;
            let track = hits.first().context("no results")?;
            println!("{} - {} ({})", track.artist, track.title, track.id.key());
            match services.yt_video_for(track).await? {
                Some(id) => println!("video: https://www.youtube.com/watch?v={id}"),
                None => println!("no video"),
            }
        }
        "yt-related" => {
            for t in services.yt_related(arg(1)?).await? {
                println!(
                    "{}\t{} - {} [{}]\t{}ms",
                    t.id.key(),
                    t.artist,
                    t.title,
                    t.album,
                    t.duration
                );
            }
        }
        "yt-search" => {
            for t in services.yt_search(arg(1)?).await? {
                println!(
                    "{}\t{} - {}\t{}ms",
                    t.id.key(),
                    t.artist,
                    t.title,
                    t.duration
                );
            }
        }
        "sp-search" => {
            let r = services
                .spotify_search(arg(1)?, mosaic_services::Services::SEARCH_LIMIT)
                .await?;
            for t in &r.tracks {
                println!(
                    "{}\t{} - {}\t{}ms",
                    t.id.key(),
                    t.artist,
                    t.title,
                    t.duration
                );
            }
            println!(
                "{} tracks, {} artists, {} albums, {} playlists",
                r.tracks.len(),
                r.artists.len(),
                r.albums.len(),
                r.playlists.len()
            );
        }
        "sp-artist" => {
            let a = services.spotify_artist(arg(1)?).await?;
            println!(
                "{}: {} top, {} albums",
                a.artist.name,
                a.top.len(),
                a.albums.len()
            );
            for t in &a.top {
                println!("  {} - {}", t.artist, t.title);
            }
            for al in &a.albums {
                println!("  [{}]", al.name);
            }
        }
        "sp-raw" => {
            let q: Vec<(&str, String)> = cmd[2..]
                .iter()
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k, v.to_string()))
                .collect();
            match services.spotify_api(arg(1)?, &q).await {
                Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
                Err(e) => println!("ERROR: {e}"),
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
        "play" => play(mosaic_audio::SourceSpec::File(arg(1)?.into()), 0.0)?,
        "sc-play" => {
            let id = TrackId::Num(arg(1)?.parse()?);
            let start = cmd.get(2).map_or(Ok(0.0), |v| v.parse::<f64>())?;
            let stream = services.sc_stream(&id).await?;
            let source = match stream.kind {
                StreamKind::Progressive => mosaic_audio::SourceSpec::Http { url: stream.url },
                StreamKind::Hls(_) => mosaic_audio::SourceSpec::Hls {
                    playlist_url: stream.url,
                },
            };
            tokio::task::spawn_blocking(move || play(source, start)).await??;
        }
        "queue-play" => {
            let ids = (1..cmd.len())
                .map(|i| Ok(TrackId::Num(arg(i)?.parse()?)))
                .collect::<Result<Vec<_>>>()?;
            queue_play(services, ids).await?;
        }
        other => bail!("unknown debug command `{other}`"),
    }
    Ok(())
}

fn play(source: mosaic_audio::SourceSpec, start_at: f64) -> Result<()> {
    use mosaic_audio::{Command, DeckId, Engine, Event};

    let engine = Engine::start()?;
    engine.send(Command::Load {
        deck: DeckId::A,
        gen: 1,
        source,
        start_at,
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

/// Plays a SoundCloud queue through the real controller with a 6 s crossfade.
async fn queue_play(services: Services, ids: Vec<TrackId>) -> Result<()> {
    use std::time::Instant;

    use mosaic_audio::Engine;
    use mosaic_core::model::Settings;

    use crate::player::{Effect, PlayerController, PlayerInput};
    use crate::resolve::resolve_stream;

    let tracks = ids.iter().map(stub_track).collect::<Result<Vec<_>>>()?;
    let first = tracks.first().cloned().context("need at least one id")?;

    let engine = Engine::start()?;
    let (tx, rx) = crossbeam_channel::unbounded::<PlayerInput>();
    {
        let (events, events_tx) = (engine.events(), tx.clone());
        std::thread::spawn(move || {
            for e in events.iter() {
                if events_tx.send(PlayerInput::Engine(e)).is_err() {
                    break;
                }
            }
        });
        let tick_tx = tx.clone();
        std::thread::spawn(move || {
            while tick_tx.send(PlayerInput::Tick).is_ok() {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        });
    }

    let mut controller = PlayerController::new(Settings {
        crossfade: 6.0,
        ..Settings::default()
    });
    for effect in controller.sync_engine() {
        if let Effect::Engine(c) = effect {
            engine.send(c);
        }
    }
    let mut pending = vec![PlayerInput::Play(first, Some(tracks))];
    let mut outstanding = 0usize;
    let mut last_print = -10.0;
    loop {
        let input = match pending.pop() {
            Some(i) => i,
            None => rx.recv()?,
        };
        let mut done = false;
        match &input {
            PlayerInput::StreamResolved { .. } | PlayerInput::Related { .. } => {
                outstanding = outstanding.saturating_sub(1)
            }
            PlayerInput::Engine(mosaic_audio::Event::Position { secs, .. })
                if secs - last_print >= 1.0 =>
            {
                println!("position {secs:.1}s");
                last_print = *secs;
            }
            PlayerInput::Engine(mosaic_audio::Event::Ended { .. }) => done = true,
            _ => {}
        }
        for effect in controller.handle(input, Instant::now()) {
            match effect {
                Effect::Engine(c) => engine.send(c),
                Effect::ResolveStream {
                    track,
                    gen,
                    seek_to,
                } => {
                    outstanding += 1;
                    done = false;
                    let (services, tx) = (services.clone(), tx.clone());
                    tokio::spawn(async move {
                        let result = resolve_stream(&services, &track, seek_to).await;
                        let _ = tx.send(PlayerInput::StreamResolved { gen, result });
                    });
                }
                Effect::FetchRelated { seed, purpose } => {
                    outstanding += 1;
                    done = false;
                    let (services, tx) = (services.clone(), tx.clone());
                    tokio::spawn(async move {
                        let result = services
                            .sc_related(&seed.id, 40)
                            .await
                            .map_err(|e| e.to_string());
                        let _ = tx.send(PlayerInput::Related {
                            purpose,
                            seed,
                            result,
                        });
                    });
                }
                Effect::Toast(message) => println!("toast: {message}"),
                _ => {}
            }
        }
        if !controller.error.is_empty() {
            bail!("{}", controller.error);
        }
        if done && outstanding == 0 {
            return Ok(());
        }
    }
}

fn stub_track(id: &TrackId) -> Result<mosaic_core::model::Track> {
    Ok(serde_json::from_value(serde_json::json!({
        "id": id, "source": "soundcloud", "title": id.key(), "artist": "",
    }))?)
}
