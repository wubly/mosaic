use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct Args {
    pub version: bool,
    pub data_dir: Option<PathBuf>,
    pub reset_window: bool,
    pub youtube_signin: bool,
    pub debug_theme: Option<String>,
    pub debug_screenshot: Option<PathBuf>,
    pub debug_view: Option<String>,
    pub debug_import: Option<String>,
    pub debug_cmd: Option<Vec<String>>,
    pub files: Vec<PathBuf>,
}

impl Args {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Args {
        let mut out = Args::default();
        let mut it = args.into_iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--version" | "-V" => out.version = true,
                "--reset-window" => out.reset_window = true,
                "--youtube-signin" => out.youtube_signin = true,
                "--debug-screenshot" => out.debug_screenshot = it.next().map(PathBuf::from),
                "--debug-view" => out.debug_view = it.next(),
                "--debug-import" => out.debug_import = it.next(),
                "--debug-theme" => out.debug_theme = it.next(),
                "--data-dir" => out.data_dir = it.next().map(PathBuf::from),
                "--debug-cmd" => {
                    out.debug_cmd = Some(it.by_ref().collect());
                    break;
                }
                other if !other.starts_with("--") => out.files.push(PathBuf::from(other)),
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Args {
        Args::parse(s.split_whitespace().map(String::from))
    }

    #[test]
    fn parses_flags() {
        let a = parse("--data-dir /tmp/x --reset-window a.mp3");
        assert_eq!(a.data_dir, Some(PathBuf::from("/tmp/x")));
        assert!(a.reset_window);
        assert_eq!(a.files, [PathBuf::from("a.mp3")]);
        assert!(parse("--version").version);
    }

    #[test]
    fn parses_youtube_signin() {
        assert!(parse("--youtube-signin").youtube_signin);
        assert!(!parse("--version").youtube_signin);
    }

    #[test]
    fn debug_cmd_takes_the_rest() {
        let a = parse("--debug-cmd sc-search --version");
        assert_eq!(a.debug_cmd.unwrap(), ["sc-search", "--version"]);
        assert!(!a.version);
    }
}
