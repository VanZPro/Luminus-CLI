use luminus::{
    app::App,
    tui::{Theme, render_to_string_with_composer},
};

#[test]
fn composer_text_is_visible_in_wide_and_narrow_layouts() {
    let app = App::default();
    let wide = render_to_string_with_composer(&app, 120, 40, Theme::luminus(false), "build auth");
    let narrow = render_to_string_with_composer(&app, 60, 24, Theme::luminus(true), "build auth");

    assert!(wide.contains("build auth"));
    assert!(narrow.contains("build auth"));
}
