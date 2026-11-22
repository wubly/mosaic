use iced::widget::{container, text};
use iced::{window, Color, Element, Size, Task, Theme};

const ICON: &[u8] = include_bytes!("../../assets/icon.png");

struct App {
    main: window::Id,
}

#[derive(Debug, Clone)]
enum Message {
    WindowClosed(window::Id),
}

impl App {
    fn boot() -> (Self, Task<Message>) {
        let (main, open) = window::open(window_settings());
        (Self { main }, open.discard())
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowClosed(id) if id == self.main => iced::exit(),
            Message::WindowClosed(_) => Task::none(),
        }
    }

    fn view(&self, _window: window::Id) -> Element<'_, Message> {
        container(text("Mosaic")).center(iced::Fill).into()
    }

    fn title(&self, _window: window::Id) -> String {
        "Mosaic".into()
    }

    fn theme(&self, _window: window::Id) -> Theme {
        Theme::Dark
    }

    fn style(&self, theme: &Theme) -> iced::theme::Style {
        iced::theme::Style {
            background_color: Color::from_rgb8(0x14, 0x12, 0x18),
            text_color: theme.palette().text,
        }
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        window::close_events().map(Message::WindowClosed)
    }
}

fn window_settings() -> window::Settings {
    let decorations = std::env::var_os("MOSAIC_DECORATIONS").is_some_and(|v| v == "1");
    window::Settings {
        size: Size::new(1280.0, 820.0),
        min_size: Some(Size::new(980.0, 640.0)),
        decorations,
        icon: window::icon::from_file_data(ICON, None).ok(),
        #[cfg(target_os = "linux")]
        platform_specific: window::settings::PlatformSpecific {
            application_id: "mosaic".into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn main() -> anyhow::Result<()> {
    iced::daemon(App::boot, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .style(App::style)
        .subscription(App::subscription)
        .run()?;
    Ok(())
}
