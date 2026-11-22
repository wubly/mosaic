//! System tray: StatusNotifierItem on Linux, `tray-icon` elsewhere, behind one small API.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(target_os = "linux"))]
mod other;

use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Open,
    Toggle,
    Next,
    Previous,
    Quit,
}

/// RGBA pixels of the tray icon.
#[derive(Clone)]
pub struct TrayIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub type OnAction = Arc<dyn Fn(TrayAction) + Send + Sync>;

/// The menu entries in `electron/tray.js` order; `None` is a separator.
pub const MENU: [Option<(TrayAction, &str)>; 7] = [
    Some((TrayAction::Open, "Open Mosaic")),
    None,
    Some((TrayAction::Toggle, "Play / pause")),
    Some((TrayAction::Next, "Next")),
    Some((TrayAction::Previous, "Previous")),
    None,
    Some((TrayAction::Quit, "Quit")),
];

/// A running tray icon; dropping it removes the icon.
pub struct Tray {
    _inner: Inner,
}

#[cfg(target_os = "linux")]
type Inner = linux::Handle;
#[cfg(not(target_os = "linux"))]
type Inner = other::Handle;

/// `Err` when there is no tray to show it in (GNOME without the AppIndicator extension, no
/// StatusNotifierWatcher on the bus, ...).
pub fn start(icon: &TrayIcon, on_action: OnAction) -> Result<Tray, String> {
    #[cfg(target_os = "linux")]
    let inner = linux::start(icon, on_action)?;
    #[cfg(not(target_os = "linux"))]
    let inner = other::start(icon, on_action)?;
    Ok(Tray { _inner: inner })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_matches_the_electron_tray() {
        let labels: Vec<_> = MENU.iter().map(|m| m.map(|(_, l)| l)).collect();
        assert_eq!(
            labels,
            [
                Some("Open Mosaic"),
                None,
                Some("Play / pause"),
                Some("Next"),
                Some("Previous"),
                None,
                Some("Quit")
            ]
        );
    }
}
