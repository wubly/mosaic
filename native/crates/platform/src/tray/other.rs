use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use super::{OnAction, TrayAction, TrayIcon, MENU};

pub struct Handle {
    _icon: tray_icon::TrayIcon,
}

pub fn start(icon: &TrayIcon, on_action: OnAction) -> Result<Handle, String> {
    let menu = Menu::new();
    let mut actions = Vec::new();
    for entry in MENU {
        match entry {
            None => menu
                .append(&PredefinedMenuItem::separator())
                .map_err(|e| e.to_string())?,
            Some((action, label)) => {
                let item = MenuItem::new(label, true, None);
                menu.append(&item).map_err(|e| e.to_string())?;
                actions.push((item.id().clone(), action));
            }
        }
    }
    let image =
        Icon::from_rgba(icon.rgba.clone(), icon.width, icon.height).map_err(|e| e.to_string())?;
    let tray = TrayIconBuilder::new()
        .with_tooltip("Mosaic")
        .with_icon(image)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| e.to_string())?;

    let menu_actions = on_action.clone();
    std::thread::spawn(move || {
        for event in MenuEvent::receiver() {
            if let Some((_, action)) = actions.iter().find(|(id, _)| *id == event.id) {
                menu_actions(*action);
            }
        }
    });
    std::thread::spawn(move || {
        for event in TrayIconEvent::receiver() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                on_action(TrayAction::Open);
            }
        }
    });
    Ok(Handle { _icon: tray })
}
