use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use mosaic_core::model::TrackId;
use mosaic_core::storage::{load_config, Dirs};
use mosaic_services::lyrics::service::LyricsArgs;
use mosaic_services::soundcloud::client_types::StreamKind;
use mosaic_services::Services;

fn parse_source(name: &str) -> Result<mosaic_core::model::Source> {
    use mosaic_core::model::Source;
    Ok(match name {
        "yt" | "youtube" => Source::YouTube,
        "sc" | "soundcloud" => Source::SoundCloud,
        "mono" | "monochrome" => Source::Monochrome,
        other => bail!("unknown service {other} (yt, sc, mono)"),
    })
}

fn services(dirs: Dirs) -> Result<Services> {
    let config = Arc::new(Mutex::new(load_config(&dirs)));
    Services::new(dirs, config)
}

pub fn run(dirs: Dirs, cmd: &[String]) -> Result<()> {
    // Service logs to stderr (RUST_LOG overrides), so backend trouble shows up here.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("mosaic_services=info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
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
        // The whole Google sign-in, as the Account tab runs it.
        "yt-signin" => match services.yt_sign_in().await? {
            Some(account) => println!(
                "signed in as {} ({})",
                account.name, account.email_or_handle
            ),
            None => println!("not signed in (window closed first)"),
        },
        // The tabs a Firefox session file (`recovery.jsonlz4`) holds.
        "firefox-tabs" => {
            use mosaic_services::youtube::browser;
            let data = std::fs::read(arg(1)?)?;
            let json = browser::mozlz4(&data).context("not a mozLz4 file")?;
            let session: serde_json::Value = serde_json::from_slice(&json)?;
            for url in browser::firefox_tab_urls(&session) {
                println!("{url}");
            }
        }
        "yt-browser-probe" => {
            use mosaic_services::youtube::browser;
            let found = browser::find();
            for b in &found {
                println!(
                    "found {} ({:?}) {} {:?}",
                    b.name,
                    b.engine,
                    b.program.display(),
                    b.prefix
                );
            }
            let b = found.first().context("no browser found")?;
            let dir = std::env::temp_dir().join("mosaic-browser-probe");
            println!("{}", browser::probe(services.http(), b, &dir).await?);
        }
        // Plays the first `count` song hits of a service's search through the real controller,
        // the way a click in search results does (one track, then autoplay).
        "search-play" => {
            let source = parse_source(arg(1)?)?;
            let count = cmd.get(3).map_or(Ok(1), |v| v.parse::<usize>())?;
            let found = services.search_all(&source, arg(2)?).await?;
            let tracks: Vec<_> = found.tracks.into_iter().take(count).collect();
            for t in &tracks {
                println!("queued {} - {} ({}ms)", t.artist, t.title, t.duration);
            }
            queue_play(services, tracks, 0.0).await?;
        }
        // What autoplay queues after a service's first hit for a query (no playback).
        "autoplay" => {
            let seed = if arg(1)? == "sp" {
                let r = services
                    .spotify_search(arg(2)?, mosaic_services::Services::SEARCH_LIMIT)
                    .await?;
                r.tracks.first().context("no results")?.clone()
            } else {
                let source = parse_source(arg(1)?)?;
                let found = services.search_all(&source, arg(2)?).await?;
                found.tracks.first().context("no results")?.clone()
            };
            println!("seed: {} - {}", seed.artist, seed.title);
            let list = services.autoplay(std::slice::from_ref(&seed), 50).await?;
            println!("{} tracks", list.len());
            for t in list.iter().take(8) {
                println!("  {} - {}", t.artist, t.title);
            }
        }
        "catalog-search" => {
            let source = parse_source(arg(1)?)?;
            let found = services.search_all(&source, arg(2)?).await?;
            for t in found.tracks.iter().take(5) {
                println!("song\t{} - {}\t{}ms", t.artist, t.title, t.duration);
            }
            for c in found
                .artists
                .iter()
                .chain(&found.albums)
                .chain(&found.playlists)
            {
                println!(
                    "{:?}\t{}\t{}\t{}\t{}",
                    c.kind, c.id, c.title, c.subtitle, c.image
                );
            }
        }
        "catalog-open" => {
            use mosaic_services::catalog::{Card, CardKind};
            let kind = match arg(2)? {
                "artist" => CardKind::Artist,
                "album" => CardKind::Album,
                "playlist" => CardKind::Playlist,
                other => bail!("unknown kind {other}"),
            };
            let card = Card {
                source: parse_source(arg(1)?)?,
                kind,
                id: arg(3)?.to_string(),
                title: String::new(),
                subtitle: String::new(),
                image: String::new(),
            };
            let page = services.open_card(&card).await?;
            println!(
                "{} | {} | {}",
                page.card.title, page.card.subtitle, page.card.image
            );
            if let Some(a) = &page.artist {
                println!("by {} ({})", a.title, a.id);
            }
            for t in &page.tracks {
                println!(
                    "song\t{} - {} [{}]\t{}ms\t{}",
                    t.artist,
                    t.title,
                    t.album,
                    t.duration,
                    !t.artwork.is_empty()
                );
            }
            for c in &page.albums {
                println!("album\t{}\t{}\t{}", c.id, c.title, c.subtitle);
            }
        }
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
        "mono-search" => {
            for t in services.mono_search(arg(1)?).await? {
                println!("{}	{} - {}	{}ms", t.id.key(), t.artist, t.title, t.duration);
            }
        }
        "mono-radio" => {
            let hits = services.mono_search(arg(1)?).await?;
            let seed = hits.first().context("no results")?;
            println!("seed: {} - {} ({})", seed.artist, seed.title, seed.id.key());
            for t in services.mono_related(seed, 20).await? {
                println!("{}	{} - {}	{}ms", t.id.key(), t.artist, t.title, t.duration);
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
        "http-play" => {
            let url = arg(1)?.to_string();
            tokio::task::spawn_blocking(move || play(mosaic_audio::SourceSpec::Http { url }, 0.0))
                .await??;
        }
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
            let tracks = ids.iter().map(stub_track).collect::<Result<Vec<_>>>()?;
            queue_play(services, tracks, 6.0).await?;
        }
        // Plays the first `count` Spotify search results through the real controller and
        // resolver (the UI's path): the platform's Spotify backend end to end.
        "sp-play" => {
            let count = cmd.get(2).map_or(Ok(2), |v| v.parse::<usize>())?;
            let r = services
                .spotify_search(arg(1)?, mosaic_services::Services::SEARCH_LIMIT)
                .await?;
            let tracks: Vec<_> = r.tracks.into_iter().take(count).collect();
            for t in &tracks {
                println!("queued {} - {} ({}ms)", t.artist, t.title, t.duration);
            }
            queue_play(services, tracks, 0.0).await?;
        }
        // Plays the first `count` Monochrome search results through the real controller.
        "mono-play" => {
            let count = cmd.get(2).map_or(Ok(2), |v| v.parse::<usize>())?;
            let tracks: Vec<_> = services
                .mono_search(arg(1)?)
                .await?
                .into_iter()
                .take(count)
                .collect();
            for t in &tracks {
                println!("queued {} - {} ({:?})", t.artist, t.title, t.id);
            }
            queue_play(services, tracks, 0.0).await?;
        }
        other => bail!("unknown debug command `{other}`"),
    }
    Ok(())
}

/// The debug players start muted so test runs stay silent; `MOSAIC_DEBUG_VOLUME=0.8` to listen.
fn debug_volume() -> f64 {
    std::env::var("MOSAIC_DEBUG_VOLUME")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0)
}

fn play(source: mosaic_audio::SourceSpec, start_at: f64) -> Result<()> {
    use mosaic_audio::{Command, DeckId, Engine, Event};

    let engine = Engine::start()?;
    engine.send(Command::SetVolume(debug_volume() as f32));
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

/// Plays a queue through the real controller and resolver.
async fn queue_play(
    services: Services,
    tracks: Vec<mosaic_core::model::Track>,
    crossfade: f64,
) -> Result<()> {
    use std::time::Instant;

    use mosaic_audio::Engine;
    use mosaic_core::model::Settings;

    use crate::player::{Effect, PlayerController, PlayerInput};
    use crate::resolve::resolve_stream;

    let first = tracks.first().cloned().context("need at least one track")?;

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
        crossfade,
        volume: debug_volume(),
        ..Settings::default()
    });
    for effect in controller.sync_engine() {
        if let Effect::Engine(c) = effect {
            engine.send(c);
        }
    }
    // Popped from the end: Spotify is marked available (the app does that once it's connected;
    // here the token comes with the data folder), then the queue plays.
    let mut pending = vec![
        PlayerInput::Play(first, Some(tracks)),
        PlayerInput::SpotifyAvailable(cfg!(feature = "spotify")),
        PlayerInput::YoutubeAvailable(true),
    ];
    let mut outstanding = 0usize;
    let mut last_print = -10.0;
    loop {
        let input = match pending.pop() {
            Some(i) => i,
            None => rx.recv()?,
        };
        let mut done = false;
        match &input {
            PlayerInput::StreamResolved { result, .. } => {
                outstanding = outstanding.saturating_sub(1);
                match result {
                    Ok(_) => {
                        println!("resolved");
                        // A new track starts its clock again.
                        last_print = -10.0;
                    }
                    Err(e) => println!("resolve failed: {e}"),
                }
            }
            PlayerInput::Related { result, .. } => {
                outstanding = outstanding.saturating_sub(1);
                match result {
                    Ok(list) => println!(
                        "autoplay: {} tracks: {}",
                        list.len(),
                        list.iter()
                            .take(5)
                            .map(|t| format!("{} - {}", t.artist, t.title))
                            .collect::<Vec<_>>()
                            .join(" | ")
                    ),
                    Err(e) => println!("autoplay failed: {e}"),
                }
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
        let was_related = matches!(input, PlayerInput::Related { .. });
        let effects = controller.handle(input, Instant::now());
        if was_related {
            println!(
                "queue: {} tracks, at {}",
                controller.queue.list.len(),
                controller.queue.index
            );
        }
        for effect in effects {
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
                    println!("autoplay: fetching for {} - {}", seed.artist, seed.title);
                    outstanding += 1;
                    done = false;
                    let (services, tx) = (services.clone(), tx.clone());
                    tokio::spawn(async move {
                        let result = services
                            .autoplay(std::slice::from_ref(&seed), 50)
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
