//! Events from background threads (engine, media keys, tray, other instances) funnelled into one
//! iced subscription.

use std::sync::{Mutex, OnceLock};

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

#[derive(Debug, Clone)]
pub enum External {
    Engine(mosaic_audio::Event),
}

static SENDER: OnceLock<UnboundedSender<External>> = OnceLock::new();
static RECEIVER: Mutex<Option<UnboundedReceiver<External>>> = Mutex::new(None);

pub fn sender() -> UnboundedSender<External> {
    SENDER
        .get_or_init(|| {
            let (tx, rx) = unbounded_channel();
            if let Ok(mut slot) = RECEIVER.lock() {
                *slot = Some(rx);
            }
            tx
        })
        .clone()
}

/// Taken once by the subscription; later calls get `None`.
pub fn take_receiver() -> Option<UnboundedReceiver<External>> {
    sender();
    RECEIVER.lock().ok()?.take()
}
