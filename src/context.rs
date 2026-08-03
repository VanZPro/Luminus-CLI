//! Deterministic context-window accounting.
//!
//! Token estimation intentionally uses whitespace-separated words rather than a
//! model tokenizer. This is stable, dependency-free, and suitable for deciding
//! when to compact; callers with exact tokenizer counts can pass those counts to
//! [`ContextBudget::add_usage`].

use std::fmt;

/// A non-negative token count.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct TokenCount(pub usize);

impl From<usize> for TokenCount {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

impl From<TokenCount> for usize {
    fn from(value: TokenCount) -> Self {
        value.0
    }
}

impl fmt::Display for TokenCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Tracks prompt usage while reserving room for the model response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextBudget {
    context_window: TokenCount,
    reserved_response_tokens: TokenCount,
    used_tokens: TokenCount,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextBudgetError {
    pub requested: TokenCount,
    pub available: TokenCount,
}

impl ContextBudget {
    pub fn new(context_window: usize, reserved_response_tokens: usize) -> Self {
        Self {
            context_window: context_window.into(),
            reserved_response_tokens: reserved_response_tokens.into(),
            used_tokens: TokenCount(0),
        }
    }

    pub fn context_window(&self) -> TokenCount {
        self.context_window
    }
    pub fn reserved_response_tokens(&self) -> TokenCount {
        self.reserved_response_tokens
    }
    pub fn used_tokens(&self) -> TokenCount {
        self.used_tokens
    }

    pub fn available_tokens(&self) -> TokenCount {
        TokenCount(
            self.context_window
                .0
                .saturating_sub(self.reserved_response_tokens.0)
                .saturating_sub(self.used_tokens.0),
        )
    }

    /// Usage as a percentage of the usable (non-reserved) context.
    pub fn percentage(&self) -> f64 {
        let capacity = self
            .context_window
            .0
            .saturating_sub(self.reserved_response_tokens.0);
        if capacity == 0 {
            0.0
        } else {
            self.used_tokens.0 as f64 / capacity as f64 * 100.0
        }
    }

    pub fn can_fit(&self, tokens: usize) -> bool {
        tokens <= self.available_tokens().0
    }

    pub fn add_usage(&mut self, tokens: usize) -> Result<(), ContextBudgetError> {
        if self.can_fit(tokens) {
            self.used_tokens.0 += tokens;
            Ok(())
        } else {
            Err(ContextBudgetError {
                requested: tokens.into(),
                available: self.available_tokens(),
            })
        }
    }

    /// Account for a user's prompt using the deterministic token estimator.
    pub fn account_user_prompt(&mut self, prompt: &str) -> Result<(), ContextBudgetError> {
        self.add_usage(Self::estimate_tokens(prompt).0)
    }

    /// Account for an incremental assistant response (including streamed deltas).
    pub fn account_assistant_delta(&mut self, delta: &str) -> Result<(), ContextBudgetError> {
        self.add_usage(Self::estimate_tokens(delta).0)
    }

    /// Replace the response reservation for the current request.
    pub fn reserve_response(&mut self, tokens: usize) {
        self.reserved_response_tokens = tokens.into();
    }

    /// Clear usage after a request completes.
    pub fn complete_request(&mut self) {
        self.reset();
    }

    /// Clear usage after a request is cancelled.
    pub fn cancel_request(&mut self) {
        self.reset();
    }

    pub fn reset(&mut self) {
        self.used_tokens = TokenCount(0);
    }

    /// Remove compacted tokens from the current usage.
    pub fn compact(&mut self, reclaimed_tokens: usize) -> Result<(), ContextBudgetError> {
        if reclaimed_tokens <= self.used_tokens.0 {
            self.used_tokens.0 -= reclaimed_tokens;
            Ok(())
        } else {
            Err(ContextBudgetError {
                requested: reclaimed_tokens.into(),
                available: self.used_tokens,
            })
        }
    }

    pub fn estimate_tokens(text: &str) -> TokenCount {
        TokenCount(text.split_whitespace().count())
    }
}
