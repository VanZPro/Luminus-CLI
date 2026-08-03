#[path = "../src/tool_activity.rs"]
mod tool_activity;

use std::time::Duration;
use tool_activity::{CARD_DETAIL_LIMIT, ToolActivity, ToolStatus, truncate};

#[test]
fn lifecycle_events_have_typed_status_and_stable_cards() {
    let started = ToolActivity::started("shell");
    assert_eq!(started.meta().status, ToolStatus::Started);
    assert_eq!(started.card(), "[started] shell");

    let progress = ToolActivity::progress("shell", "running");
    assert_eq!(progress.meta().status, ToolStatus::InProgress);
    assert_eq!(progress.card(), "[progress] shell\nrunning");

    let completed = ToolActivity::completed("shell", "ok").with_duration(Duration::from_millis(42));
    assert_eq!(completed.meta().status, ToolStatus::Completed);
    assert_eq!(completed.card(), "[completed] shell (42ms)\nok");

    let failed = ToolActivity::failed("shell", "nope");
    assert_eq!(failed.meta().status, ToolStatus::Failed);
    assert_eq!(failed.card(), "[failed] shell\nnope");
}

#[test]
fn card_detail_is_safely_bounded_and_unicode_safe() {
    let text = "x".repeat(CARD_DETAIL_LIMIT + 10);
    let card = ToolActivity::completed("tool", text).card();
    let detail = card.split_once('\n').unwrap().1;
    assert_eq!(detail.chars().count(), CARD_DETAIL_LIMIT);
    assert!(detail.ends_with('…'));
    assert_eq!(truncate("😀abcdef", 4), "😀ab…");
    assert_eq!(truncate("abcdef", 0), "");
}
