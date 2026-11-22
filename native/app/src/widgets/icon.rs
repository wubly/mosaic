use iced::widget::{text, Text};
use iced::{alignment, Font, Length};

use super::icon_codes::CODES;

pub const FILLED: Font = Font::with_name("Mosaic Symbols Filled");
pub const OUTLINED: Font = Font::with_name("Mosaic Symbols Outlined");

macro_rules! icons {
    ($($variant:ident => $name:literal),* $(,)?) => {
        /// One variant per `react-icons/md` glyph the JS uses (plus the window buttons).
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon { $($variant),* }

        impl Icon {
            pub const ALL: &'static [Icon] = &[$(Icon::$variant),*];

            pub fn name(self) -> &'static str {
                match self { $(Icon::$variant => $name),* }
            }
        }
    };
}

icons! {
    Add => "add", Album => "album", ArrowBack => "arrow_back", ArrowForward => "arrow_forward",
    AudioFile => "audio_file", AutoAwesome => "auto_awesome", Bedtime => "bedtime", Bolt => "bolt",
    Check => "check", CheckBox => "check_box", CheckBoxBlank => "check_box_outline_blank",
    ChevronRight => "chevron_right", ClearAll => "clear_all", Close => "close",
    CloudDownload => "cloud_download", CreateNewFolder => "create_new_folder", Delete => "delete",
    DragIndicator => "drag_indicator", Edit => "edit", ExpandLess => "expand_less",
    ExpandMore => "expand_more", Explore => "explore", Favorite => "favorite", Folder => "folder",
    FolderOpen => "folder_open", Fullscreen => "fullscreen", GraphicEq => "graphic_eq",
    History => "history", ArrowDown => "keyboard_arrow_down", LibraryMusic => "library_music",
    Link => "link", Fire => "local_fire_department", Lock => "lock", Lyrics => "lyrics",
    MoreVert => "more_vert", MusicNote => "music_note", OpenInFull => "open_in_full",
    OpenInNew => "open_in_new", Pause => "pause", Person => "person",
    PictureInPicture => "picture_in_picture_alt", PlayArrow => "play_arrow",
    PlaylistAdd => "playlist_add", PlaylistPlay => "playlist_play", QueueMusic => "queue_music",
    Refresh => "refresh", Repeat => "repeat", RepeatOne => "repeat_one",
    RestartAlt => "restart_alt", Search => "search", SearchOff => "search_off",
    Sensors => "sensors", Settings => "settings", Shuffle => "shuffle", SkipNext => "skip_next",
    SkipPrevious => "skip_previous", Subtitles => "subtitles", Tune => "tune",
    UploadFile => "upload_file", VolumeDown => "volume_down", VolumeOff => "volume_off",
    VolumeUp => "volume_up", Minimize => "remove", Maximize => "crop_square",
    Restore => "filter_none", Sync => "sync", Warning => "warning",
}

impl Icon {
    pub fn glyph(self) -> char {
        CODES
            .iter()
            .find(|(name, _)| *name == self.name())
            .map_or('\u{fffd}', |(_, c)| *c)
    }
}

fn glyph<'a>(icon: Icon, size: f32, font: Font) -> Text<'a> {
    text(icon.glyph().to_string())
        .font(font)
        .size(size)
        .line_height(1.0)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Center)
}

/// The solid glyph (`MdX` in react-icons).
pub fn icon<'a>(icon: Icon, size: f32) -> Text<'a> {
    glyph(icon, size, FILLED)
}

/// The outlined glyph (`MdOutlineX`).
pub fn outlined<'a>(icon: Icon, size: f32) -> Text<'a> {
    glyph(icon, size, OUTLINED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_a_codepoint() {
        for icon in Icon::ALL {
            assert_ne!(
                icon.glyph(),
                '\u{fffd}',
                "{icon:?} ({}) is not in the font table",
                icon.name()
            );
        }
    }
}
