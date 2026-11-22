use std::sync::LazyLock;
use std::time::Duration;

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use regex::Regex;
use unicode_normalization::UnicodeNormalization;

use super::parse::parse_title;
use crate::Services;

const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130 Safari/537.36";

/// The characters `encodeURIComponent` leaves alone.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static regex")
}

static SLUG_DROP: LazyLock<Regex> = LazyLock::new(|| re(r"[,._@!#%^*+:;'()\[\]?]"));
static SLUG_SPACE: LazyLock<Regex> = LazyLock::new(|| re(r"[\s/]"));
static SLUG_AMP: LazyLock<Regex> = LazyLock::new(|| re(r"-?&-?"));
static SLUG_DASHES: LazyLock<Regex> = LazyLock::new(|| re(r"-+"));
static ENTITY: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)&(#x?[0-9a-f]+|[a-z]+);"));
static DIV_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"<(/?)div\b[^>]*>"));
static LYRICS_OPEN: LazyLock<Regex> =
    LazyLock::new(|| re(r#"<div[^>]*data-lyrics-container="true"[^>]*>"#));
static EXCLUDE_OPEN: LazyLock<Regex> =
    LazyLock::new(|| re(r#"<div[^>]*data-exclude-from-selection="true"[^>]*>"#));
static BR: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)<br\s*/?>"));
static ANY_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"<[^>]+>"));
static BRACKETED: LazyLock<Regex> = LazyLock::new(|| re(r"\[[^\]]*\]"));
static JUNK: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(\d+\s+contributors?|.*(?-u:\b)lyrics|embed|more on genius|you might also like)$")
});
static HAS_LETTER: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)[a-z]"));
static FEAT_PAREN: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\(?feat\."));
static FEAT_SPLIT: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)feat[.:]"));

fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn ascii(s: &str) -> String {
    s.nfkd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect()
}

fn slug(artist: &str, title: &str) -> String {
    let line = format!("{artist}-{title}");
    let line = SLUG_DROP.replace_all(&line, "");
    let line = SLUG_SPACE.replace_all(&line, "-");
    let line = SLUG_AMP.replace_all(&line, "-and-");
    let line = SLUG_DASHES.replace_all(&line, "-");
    utf8_percent_encode(&ascii(&line), URI_COMPONENT)
        .to_string()
        .replace("%2F", "-")
        .replace("%2f", "-")
}

fn decode(text: &str) -> String {
    ENTITY
        .replace_all(text, |caps: &regex::Captures| {
            let e = &caps[1];
            if let Some(num) = e.strip_prefix('#') {
                let code = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16),
                    None => num.parse(),
                };
                return code
                    .ok()
                    .and_then(char::from_u32)
                    .map_or_else(|| caps[0].to_string(), String::from);
            }
            match e.to_lowercase().as_str() {
                "amp" => "&".into(),
                "lt" => "<".into(),
                "gt" => ">".into(),
                "quot" => "\"".into(),
                "apos" => "'".into(),
                "nbsp" => " ".into(),
                _ => caps[0].to_string(),
            }
        })
        .into_owned()
}

/// Position just after the `</div>` that balances an already-opened div, scanning from `from`.
/// Returns `(close_tag_start, after_close)`; both are `html.len()` when unbalanced.
fn matching_close(html: &str, from: usize) -> (usize, usize) {
    let mut depth = 1;
    let mut at = from;
    while depth > 0 {
        let Some(m) = DIV_TAG.captures_at(html, at) else {
            return (html.len(), html.len());
        };
        let whole = m.get(0).expect("group 0");
        depth += if m[1].is_empty() { 1 } else { -1 };
        at = whole.end();
        if depth == 0 {
            return (whole.start(), whole.end());
        }
    }
    (html.len(), html.len())
}

fn drop_blocks(html: &str, open: &Regex) -> String {
    let mut out = html.to_string();
    while let Some(m) = open.find(&out) {
        let (_, after) = matching_close(&out, m.end());
        out = format!("{}{}", &out[..m.start()], &out[after..]);
    }
    out
}

/// Each lyrics block is a div with nested divs, so walk to the matching close tag.
fn containers(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(m) = LYRICS_OPEN.find_at(html, at) {
        let (close_start, after) = matching_close(html, m.end());
        out.push(drop_blocks(&html[m.end()..close_start], &EXCLUDE_OPEN));
        at = after;
    }
    out
}

pub(crate) fn clean(chunks: &[String]) -> String {
    let joined = chunks
        .iter()
        .map(|c| {
            ANY_TAG
                .replace_all(&BR.replace_all(c, "\n"), "")
                .into_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let text = BRACKETED.replace_all(&decode(&joined), "").into_owned();

    let mut lines: Vec<&str> = text
        .split('\n')
        .map(str::trim)
        .filter(|l| {
            !l.is_empty() && !JUNK.is_match(l) && !(js_len(l) < 2 && !HAS_LETTER.is_match(l))
        })
        .collect();
    while lines.first().is_some_and(|l| js_len(l) < 3) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| js_len(l) < 3) {
        lines.pop();
    }
    lines.join("\n")
}

pub(crate) fn lyrics_from_html(html: &str) -> Option<String> {
    let lyrics = clean(&containers(html));
    (js_len(&lyrics) > 10).then_some(lyrics)
}

impl Services {
    async fn genius_page(&self, who: &str, song: &str) -> Option<String> {
        let res = self
            .inner
            .http
            .get(format!("https://genius.com/{}-lyrics", slug(who, song)))
            .timeout(Duration::from_secs(10))
            .header("User-Agent", UA)
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None;
        }
        lyrics_from_html(&res.text().await.ok()?)
    }

    pub async fn genius_lyrics(&self, title: &str, artist: &str) -> Option<String> {
        let parsed = parse_title(title, artist);
        let first = parsed.tracks.first().map_or(title, String::as_str);
        let song = FEAT_PAREN
            .split(first)
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if song.is_empty() {
            return None;
        }
        for name in &parsed.artists {
            let who = FEAT_SPLIT.split(name).next().unwrap_or("").trim();
            if who.is_empty() {
                continue;
            }
            if let Some(found) = self.genius_page(who, &song).await {
                return Some(found);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_match_genius_urls() {
        assert_eq!(slug("Daft Punk", "Get Lucky"), "Daft-Punk-Get-Lucky");
        assert_eq!(
            slug("Simon & Garfunkel", "The Sound of Silence"),
            "Simon-and-Garfunkel-The-Sound-of-Silence"
        );
        assert_eq!(slug("Beyoncé", "Halo"), "Beyonce-Halo");
        assert_eq!(slug("AC/DC", "T.N.T."), "AC-DC-TNT");
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(
            decode("Don&#x27;t &amp; &quot;go&quot; &#65; &bogus; &#xZZ;"),
            "Don't & \"go\" A &bogus; &#xZZ;"
        );
    }

    const PAGE: &str = include_str!("../../tests/fixtures/genius.html");

    #[test]
    fn extracts_lyrics_from_fixture() {
        let lyrics = lyrics_from_html(PAGE).expect("lyrics");
        assert_eq!(
            lyrics,
            "First line of the song\nSecond line & more\nIt's a new verse\nFinal line here"
        );
        assert!(!lyrics.contains("Contributors"));
        assert!(!lyrics.contains("[Verse"));
        assert!(!lyrics.contains("Embed"));
    }

    #[test]
    fn no_container_is_none() {
        assert_eq!(lyrics_from_html("<html><body>nothing</body></html>"), None);
    }
}
