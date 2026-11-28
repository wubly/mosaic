//! One running Mosaic: a second launch sends a message to the first and exits.

use std::io::{BufRead, BufReader, Write};

use interprocess::local_socket::traits::{ListenerExt as _, Stream as _};
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, Name, NameType, Stream, ToFsName, ToNsName,
};

pub enum Instance {
    /// This process is the first one; hand the listener to [`serve`].
    Primary(interprocess::local_socket::Listener),
    /// Another instance got the message; exit.
    Secondary,
}

fn socket_name(who: &str) -> std::io::Result<Name<'static>> {
    if GenericNamespaced::is_supported() {
        format!("mosaic-{who}.sock").to_ns_name::<GenericNamespaced>()
    } else {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, Into::into);
        dir.join(format!("mosaic-{who}.sock"))
            .to_fs_name::<GenericFilePath>()
    }
}

fn current_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default()
}

/// Tries to hand `messages` to a running instance; otherwise becomes the primary one. `scope`
/// separates instances that can't get in each other's way (another data folder): one per
/// scope, the default scope being the user's own Mosaic.
pub fn acquire(scope: Option<&str>, messages: &[String]) -> std::io::Result<Instance> {
    let who = match scope {
        Some(s) => format!("{}-{s}", current_user()),
        None => current_user(),
    };
    acquire_for(&who, messages)
}

fn acquire_for(who: &str, messages: &[String]) -> std::io::Result<Instance> {
    if let Ok(mut stream) = Stream::connect(socket_name(who)?) {
        for m in messages {
            writeln!(stream, "{m}")?;
        }
        stream.flush()?;
        return Ok(Instance::Secondary);
    }
    let listener = ListenerOptions::new()
        .name(socket_name(who)?)
        .try_overwrite(true)
        .create_sync()?;
    Ok(Instance::Primary(listener))
}

/// Calls `on_message` for every line other launches send, on a background thread.
pub fn serve(
    listener: interprocess::local_socket::Listener,
    on_message: impl Fn(String) + Send + 'static,
) {
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            for line in BufReader::new(conn).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    on_message(line.trim().to_string());
                }
            }
        }
    });
}

pub const FOCUS: &str = "focus";

pub fn open_message(path: &str) -> String {
    format!("open {path}")
}

pub fn parse_open(message: &str) -> Option<&str> {
    message.strip_prefix("open ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_launch_hands_its_messages_to_the_first() {
        let who = format!("test-{}", std::process::id());
        let Instance::Primary(listener) = acquire_for(&who, &[]).unwrap() else {
            panic!("first launch must be primary");
        };
        let (tx, rx) = std::sync::mpsc::channel();
        serve(listener, move |m| {
            let _ = tx.send(m);
        });
        let messages = [FOCUS.to_string(), open_message("/m/a.mp3")];
        assert!(matches!(
            acquire_for(&who, &messages).unwrap(),
            Instance::Secondary
        ));
        let timeout = std::time::Duration::from_secs(2);
        assert_eq!(rx.recv_timeout(timeout).unwrap(), "focus");
        assert_eq!(rx.recv_timeout(timeout).unwrap(), "open /m/a.mp3");
    }

    #[test]
    fn open_messages_round_trip() {
        assert_eq!(parse_open(&open_message("/m/a b.mp3")), Some("/m/a b.mp3"));
        assert_eq!(parse_open(FOCUS), None);
    }
}
