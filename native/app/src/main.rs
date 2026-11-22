// Library pieces (styles, icons, animation helpers) are consumed by views added in M7-M12; this
// allowance is removed in M13.
#![allow(dead_code)]

mod ambient;
mod anim;
mod app;
mod args;
#[cfg(feature = "debug-cli")]
mod debug_cli;
mod external;
mod images;
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

    let _ = migrate_legacy(&dirs);
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
        Err(_) => (None, Some("No audio output device found".to_string())),
    };
    let media = mosaic_platform::media_controls::Media::new(None, |cmd| {
        let _ = external::sender().send(external::External::Media(cmd));
    })
    .ok();
    if let Ok(mut flags) = FLAGS.lock() {
        *flags = Some(Flags {
            dirs,
            config,
            store,
            services,
            engine,
            engine_error,
            media,
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
