mod ambient;
mod anim;
mod app;
mod args;
#[cfg(feature = "debug-cli")]
mod debug_cli;
mod external;
mod images;
mod logging;
mod player;
mod resolve;
mod text_measure;
mod theme;
mod views;
mod widgets;

use std::sync::{Arc, Mutex};

use app::{App, Flags, FLAGS};
use mosaic_audio::Engine;
use mosaic_core::storage::{load_config, load_store, migrate_legacy, Dirs};
use mosaic_services::Services;

fn main() -> anyhow::Result<()> {
    let args = args::Args::parse(std::env::args().skip(1));
    if args.version {
        println!("mosaic {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let dirs = match &args.data_dir {
        Some(root) => Dirs::with_root(root),
        None => Dirs::resolve()?,
    };
    #[cfg(feature = "debug-cli")]
    if let Some(cmd) = &args.debug_cmd {
        return debug_cli::run(dirs, cmd);
    }

    let messages: Vec<String> = if args.files.is_empty() {
        vec![mosaic_platform::single_instance::FOCUS.to_string()]
    } else {
        args.files
            .iter()
            .map(|f| mosaic_platform::single_instance::open_message(&f.to_string_lossy()))
            .collect()
    };
    match mosaic_platform::single_instance::acquire(&messages) {
        Ok(mosaic_platform::single_instance::Instance::Secondary) => return Ok(()),
        Ok(mosaic_platform::single_instance::Instance::Primary(listener)) => {
            mosaic_platform::single_instance::serve(listener, |m| {
                let _ = external::sender().send(external::External::Instance(m));
            });
        }
        Err(_) => {}
    }

    logging::init(&dirs);
    let migrated = migrate_legacy(&dirs).ok().flatten();
    let config = Arc::new(Mutex::new(load_config(&dirs)));
    let store = load_store(&dirs);
    let services = Services::new(dirs.clone(), Arc::clone(&config))?;
    let (engine, engine_error) = match Engine::start() {
        Ok(engine) => {
            let events = engine.events();
            std::thread::spawn(move || {
                let tx = external::sender();
                for event in events.iter() {
                    if tx.send(external::External::Engine(event)).is_err() {
                        break;
                    }
                }
            });
            (Some(engine), None)
        }
        Err(e) => {
            tracing::warn!("audio output unavailable: {e}");
            (None, Some("No audio output device found".to_string()))
        }
    };
    let media = mosaic_platform::media_controls::Media::new(None, |cmd| {
        let _ = external::sender().send(external::External::Media(cmd));
    })
    .ok();
    let tray = start_tray();
    if let Ok(mut flags) = FLAGS.lock() {
        *flags = Some(Flags {
            dirs,
            store,
            services,
            engine,
            engine_error,
            media,
            tray,
            migrated: migrated.is_some(),
            reset_window: args.reset_window,
            files: args.files,
            debug_theme: args.debug_theme,
            debug_screenshot: args.debug_screenshot,
            debug_view: args.debug_view,
            debug_import: args.debug_import,
        });
    }
    iced::daemon(App::boot, App::update, App::view)
        .font(include_bytes!("../../assets/fonts/RobotoFlex.ttf").as_slice())
        .font(include_bytes!("../../assets/fonts/MosaicSymbols-filled.ttf").as_slice())
        .font(include_bytes!("../../assets/fonts/MosaicSymbols-outlined.ttf").as_slice())
        .default_font(iced::Font::with_name("Roboto Flex"))
        .title(App::title)
        .theme(App::theme)
        .style(App::style)
        .subscription(App::subscription)
        .run()?;
    Ok(())
}

fn start_tray() -> Option<mosaic_platform::tray::Tray> {
    let png = image::load_from_memory(include_bytes!("../../assets/icon.png")).ok()?;
    let rgba = png
        .resize(32, 32, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let icon = mosaic_platform::tray::TrayIcon {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    };
    match mosaic_platform::tray::start(
        &icon,
        std::sync::Arc::new(|action| {
            let _ = external::sender().send(external::External::Tray(action));
        }),
    ) {
        Ok(tray) => Some(tray),
        Err(e) => {
            tracing::warn!("no system tray: {e}");
            None
        }
    }
}
