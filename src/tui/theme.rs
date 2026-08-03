use ratatui::style::Color;

/// Colors and display preferences for the Luminus terminal interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub monochrome: bool,
    pub background: Color,
    pub foreground: Color,
    pub primary: Color,
    pub accent: Color,
    pub muted: Color,
    pub border: Color,
}

impl Theme {
    /// The default black-and-blue Luminus palette.
    pub const fn luminus(monochrome: bool) -> Self {
        if monochrome {
            Self {
                monochrome: true,
                background: Color::Black,
                foreground: Color::White,
                primary: Color::White,
                accent: Color::White,
                muted: Color::Gray,
                border: Color::Gray,
            }
        } else {
            Self {
                monochrome: false,
                background: Color::Black,
                foreground: Color::White,
                primary: Color::Rgb(82, 170, 255),
                accent: Color::LightBlue,
                muted: Color::DarkGray,
                border: Color::Blue,
            }
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::luminus(false)
    }
}
