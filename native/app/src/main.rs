// No console window behind the app in Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

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
    // Both `ring` and `aws-lc-rs` are linked, so rustls cannot choose one itself.
    let _ = rustls::crypto::ring::default_provider().install_default();
    // Pace presentation to the display; the fallback GLES path otherwise may not.
    #[cfg(target_os = "linux")]
    prefer_x11_without_vulkan_wayland();
    if std::env::var_os("ICED_PRESENT_MODE").is_none() {
        std::env::set_var("ICED_PRESENT_MODE", "fifo");
    }
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
    // macOS wants the status item and Now Playing created on the main thread once the event
    // loop is running, and Windows SMTC needs the window's HWND, so there both are created
    // when the main window has opened (`Message::PlatformReady`).
    #[cfg(target_os = "linux")]
    let (media, tray) = (start_media(None), start_tray());
    #[cfg(not(target_os = "linux"))]
    let (media, tray) = (None, None);
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

/// `hwnd` is the main window's handle on Windows, where SMTC refuses to start without one.
fn start_media(hwnd: Option<isize>) -> Option<mosaic_platform::media_controls::Media> {
    if cfg!(windows) && hwnd.is_none() {
        return None;
    }
    let hwnd = hwnd.map(|h| mosaic_platform::media_controls::Hwnd(h as *mut std::ffi::c_void));
    mosaic_platform::media_controls::Media::new(hwnd, |cmd| {
        let _ = external::sender().send(external::External::Media(cmd));
    })
    .map_err(|e| tracing::warn!("no media controls: {e}"))
    .ok()
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

/// A Vulkan loader built without Wayland WSI leaves wgpu on a GLES path that cannot present
/// natively on Wayland and crawls. When that is the case and XWayland is there, run through
/// X11 instead so wgpu gets a Vulkan surface. `MOSAIC_X11=1` forces X11, `=0` forbids it.
#[cfg(target_os = "linux")]
fn prefer_x11_without_vulkan_wayland() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() || std::env::var_os("DISPLAY").is_none() {
        return;
    }
    let force = std::env::var("MOSAIC_X11").ok();
    let use_x11 = match force.as_deref() {
        Some("0") => false,
        Some(_) => true,
        None => vulkan_lacks_wayland(),
    };
    if use_x11 {
        std::env::remove_var("WAYLAND_DISPLAY");
    }
}

/// True when a Vulkan loader exists but does not offer `VK_KHR_wayland_surface`.
#[cfg(target_os = "linux")]
fn vulkan_lacks_wayland() -> bool {
    // SAFETY: loading the system Vulkan loader and enumerating instance extensions has no
    // preconditions; the entry is dropped before wgpu loads its own.
    let Ok(entry) = (unsafe { ash::Entry::load() }) else {
        return false;
    };
    let Ok(exts) = (unsafe { entry.enumerate_instance_extension_properties(None) }) else {
        return false;
    };
    !exts.iter().any(|e| {
        e.extension_name_as_c_str()
            .is_ok_and(|n| n.to_bytes() == b"VK_KHR_wayland_surface")
    })
}
