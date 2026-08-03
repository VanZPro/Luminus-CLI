/// Wide wordmark displayed when enough horizontal space is available.
pub const WIDE_LOGO: &str = "LUMINUS";
/// Compact wordmark for narrow terminal windows.
pub const COMPACT_LOGO: &str = "◆ LUMINUS";
pub const TAGLINE: &str = "Illuminate the codebase.";

pub const fn title(width: u16) -> &'static str {
    if width < 70 { COMPACT_LOGO } else { WIDE_LOGO }
}
