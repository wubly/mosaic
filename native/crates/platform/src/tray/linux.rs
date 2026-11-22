use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};

use super::{OnAction, TrayAction, TrayIcon, MENU};

struct Item {
    icon: ksni::Icon,
    on_action: OnAction,
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        "mosaic".into()
    }

    fn title(&self) -> String {
        "Mosaic".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![self.icon.clone()]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        (self.on_action)(TrayAction::Open);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        MENU.iter()
            .map(|entry| match entry {
                None => MenuItem::Separator,
                Some((action, label)) => {
                    let action = *action;
                    StandardItem {
                        label: (*label).into(),
                        activate: Box::new(move |this: &mut Item| (this.on_action)(action)),
                        ..Default::default()
                    }
                    .into()
                }
            })
            .collect()
    }
}

pub struct Handle(ksni::blocking::Handle<Item>);

impl Drop for Handle {
    fn drop(&mut self) {
        self.0.shutdown().wait();
    }
}

/// StatusNotifierItem wants ARGB32 in network byte order.
fn argb(icon: &TrayIcon) -> ksni::Icon {
    let mut data = Vec::with_capacity(icon.rgba.len());
    for px in icon.rgba.as_chunks::<4>().0 {
        data.extend_from_slice(&[px[3], px[0], px[1], px[2]]);
    }
    ksni::Icon {
        width: icon.width as i32,
        height: icon.height as i32,
        data,
    }
}

pub fn start(icon: &TrayIcon, on_action: OnAction) -> Result<Handle, String> {
    Item {
        icon: argb(icon),
        on_action,
    }
    .spawn()
    .map(Handle)
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_become_argb() {
        let icon = TrayIcon {
            width: 1,
            height: 1,
            rgba: vec![10, 20, 30, 255],
        };
        let a = argb(&icon);
        assert_eq!(a.data, [255, 10, 20, 30]);
        assert_eq!((a.width, a.height), (1, 1));
    }
}
