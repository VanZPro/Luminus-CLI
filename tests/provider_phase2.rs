use std::time::Duration;

use luminus::{
    event::ProviderEvent,
    provider::{FakeProvider, ModelInfo, Provider, ProviderCapabilities},
};
use tokio_util::sync::CancellationToken;

#[test]
fn fake_provider_exposes_deterministic_model_contract() {
    let provider = FakeProvider::new(Duration::ZERO);
    assert_eq!(provider.model(), ModelInfo::fake());
    assert_eq!(provider.capabilities(), ProviderCapabilities::fake());
    assert_eq!(provider.model().id, "fake-model");
}

#[tokio::test]
async fn fake_provider_emits_multiple_deltas_in_order() {
    let events = FakeProvider::new(Duration::ZERO)
        .stream("r".into(), "one two three".into(), CancellationToken::new())
        .await;
    let chunks: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ProviderEvent::Delta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(chunks, vec!["one", "two", "three"]);
}

#[tokio::test]
async fn fake_provider_cancellation_stops_delta_stream() {
    let token = CancellationToken::new();
    token.cancel();
    let events = FakeProvider::new(Duration::ZERO)
        .stream("r".into(), "one two".into(), token)
        .await;
    assert!(
        events
            .iter()
            .all(|e| !matches!(e, ProviderEvent::Delta { .. }))
    );
    assert!(matches!(
        events.last(),
        Some(ProviderEvent::Cancelled { .. })
    ));
}

#[test]
fn model_info_and_capabilities_are_cloneable_and_debuggable() {
    let info = ModelInfo::fake();
    let caps = ProviderCapabilities::fake();
    assert_eq!(info.clone(), info);
    assert_eq!(caps.clone(), caps);
    let _ = format!("{info:?} {caps:?}");
}

#[test]
fn provider_error_is_typed() {
    let error = luminus::provider::ProviderError::Cancelled;
    assert_eq!(error.to_string(), "provider request cancelled");
}
