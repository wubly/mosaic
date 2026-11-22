/// Opens `url` in the default browser. Only `https://` is allowed (port of `openExternalHttps`).
pub fn open_https(url: &str) -> bool {
    is_https(url) && open::that_detached(url).is_ok()
}

fn is_https(url: &str) -> bool {
    url.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_is_allowed() {
        assert!(is_https("https://soundcloud.com/a/b"));
        assert!(!is_https("http://example.com"));
        assert!(!is_https("file:///etc/passwd"));
        assert!(!is_https("javascript:alert(1)"));
        assert!(!open_https("ftp://x"));
    }
}
