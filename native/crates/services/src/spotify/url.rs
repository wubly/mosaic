use std::sync::LazyLock;

use regex::Regex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Playlist,
    Album,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Playlist => "playlist",
            Kind::Album => "album",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotifyRef {
    pub kind: Kind,
    pub id: String,
}

static WEB: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"open\.spotify\.com/(?:intl-[a-z-]+/)?(playlist|album)/([A-Za-z0-9]+)")
        .expect("static regex")
});
static URI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"spotify:(playlist|album):([A-Za-z0-9]+)").expect("static regex"));

pub fn parse_spotify_url(input: &str) -> Option<SpotifyRef> {
    let caps = WEB.captures(input).or_else(|| URI.captures(input))?;
    Some(SpotifyRef {
        kind: if &caps[1] == "playlist" {
            Kind::Playlist
        } else {
            Kind::Album
        },
        id: caps[2].to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_web_and_uri_forms() {
        let r =
            parse_spotify_url("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc")
                .unwrap();
        assert_eq!(
            (r.kind, r.id.as_str()),
            (Kind::Playlist, "37i9dQZF1DXcBWIGoYBM5M")
        );
        let r = parse_spotify_url("https://open.spotify.com/intl-de/album/4aawyAB9vmqN3uQ7FjRGTy")
            .unwrap();
        assert_eq!(r.kind, Kind::Album);
        let r = parse_spotify_url("spotify:playlist:abc123").unwrap();
        assert_eq!(r.id, "abc123");
        assert_eq!(parse_spotify_url("https://example.com/playlist/abc"), None);
        assert_eq!(parse_spotify_url("spotify:track:abc"), None);
        assert_eq!(parse_spotify_url(""), None);
    }
}
