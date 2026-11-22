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
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    tune_malloc();
    let _ = rustls::crypto::ring::default_provider().install_default();
    #[cfg(target_os = "linux")]
    pick_graphics_backend();
    // Pace presentation to the display; the fallback GLES path otherwise may not.
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
        .executor::<Executor>()
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

/// Iced's tokio runtime, sized for a desktop app rather than a server: the default starts a
/// worker per core and lets up to 512 blocking threads pile up, and every thread that allocates
/// gets its own malloc arena that keeps its memory after the burst is over.
struct Executor(tokio::runtime::Runtime);

impl iced::Executor for Executor {
    fn new() -> Result<Self, std::io::Error> {
        let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(cores.clamp(2, 4))
            // Room for the scanner's 4 workers, image decodes and a waiting Spotify sign-in.
            .max_blocking_threads(12)
            .enable_all()
            .build()
            .map(Executor)
    }

    fn spawn(&self, future: impl std::future::Future<Output = ()> + Send + 'static) {
        drop(self.0.spawn(future));
    }

    fn block_on<T>(&self, future: impl std::future::Future<Output = T>) -> T {
        self.0.block_on(future)
    }

    fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.enter();
        f()
    }
}

/// glibc gives each thread that allocates its own arena (up to 8 per core) and raises the mmap
/// threshold after a big block is freed, so decoded artwork ends up spread over dozens of arenas
/// that never shrink. A few arenas and a fixed threshold keep large buffers returning to the OS.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn tune_malloc() {
    // SAFETY: mallopt only changes allocator parameters; called before any other thread exists.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 4);
        libc::mallopt(libc::M_MMAP_THRESHOLD, 256 * 1024);
    }
}

/// Chooses the windowing and graphics backends before wgpu starts.
///
/// A Vulkan loader built without Wayland WSI leaves wgpu on a GLES path that cannot present
/// natively on Wayland and crawls. When that is the case and XWayland is there, run through
/// X11 instead so wgpu gets a Vulkan surface. `MOSAIC_X11=1` forces X11, `=0` forbids it.
///
/// When Vulkan will work, wgpu is limited to it: otherwise it also brings up EGL/GL (and on
/// many systems Mesa's LLVM) just to list adapters, around 50 MB that is never used.
/// `WGPU_BACKEND` overrides this.
#[cfg(target_os = "linux")]
fn pick_graphics_backend() {
    let vulkan = probe_vulkan();
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_some() {
        let force = std::env::var("MOSAIC_X11").ok();
        let use_x11 = match force.as_deref() {
            Some("0") => false,
            Some(_) => true,
            None => vulkan.is_some_and(|v| !v.wayland),
        };
        if use_x11 {
            std::env::remove_var("WAYLAND_DISPLAY");
        }
    }
    let on_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let vulkan_presents = vulkan.is_some_and(|v| v.gpu && (v.wayland || !on_wayland));
    if vulkan_presents && std::env::var_os("WGPU_BACKEND").is_none() {
        std::env::set_var("WGPU_BACKEND", "vulkan");
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct VulkanProbe {
    /// The loader offers `VK_KHR_wayland_surface`.
    wayland: bool,
    /// There is a hardware device (not a CPU rasterizer like lavapipe).
    gpu: bool,
}

/// What the system Vulkan loader offers, or `None` when there is no loader.
#[cfg(target_os = "linux")]
fn probe_vulkan() -> Option<VulkanProbe> {
    use ash::vk;
    // SAFETY: loading the system Vulkan loader and enumerating instance extensions has no
    // preconditions; the entry is dropped before wgpu loads its own.
    let entry = unsafe { ash::Entry::load() }.ok()?;
    let exts = unsafe { entry.enumerate_instance_extension_properties(None) }.ok()?;
    let wayland = exts.iter().any(|e| {
        e.extension_name_as_c_str()
            .is_ok_and(|n| n.to_bytes() == b"VK_KHR_wayland_surface")
    });
    let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_0);
    let info = vk::InstanceCreateInfo::default().application_info(&app);
    // SAFETY: a bare instance with no layers or extensions; it is destroyed before returning and
    // nothing created from it outlives this function.
    let gpu = unsafe {
        match entry.create_instance(&info, None) {
            Ok(instance) => {
                let gpu = instance.enumerate_physical_devices().is_ok_and(|devices| {
                    devices.iter().any(|&d| {
                        instance.get_physical_device_properties(d).device_type
                            != vk::PhysicalDeviceType::CPU
                    })
                });
                instance.destroy_instance(None);
                gpu
            }
            Err(_) => false,
        }
    };
    Some(VulkanProbe { wayland, gpu })
}
