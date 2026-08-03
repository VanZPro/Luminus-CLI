//! Provider-agnostic model selection domain types.

use std::convert::Infallible;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ModelRole {
    Default,
    Fast,
    Deep,
    Plan,
    Review,
    Subagent,
    Vision,
    Summarizer,
}

impl ModelRole {
    pub const ALL: [Self; 8] = [
        Self::Default,
        Self::Fast,
        Self::Deep,
        Self::Plan,
        Self::Review,
        Self::Subagent,
        Self::Vision,
        Self::Summarizer,
    ];
}

impl fmt::Display for ModelRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Default => "default",
            Self::Fast => "fast",
            Self::Deep => "deep",
            Self::Plan => "plan",
            Self::Review => "review",
            Self::Subagent => "subagent",
            Self::Vision => "vision",
            Self::Summarizer => "summarizer",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownRole(pub String);

impl fmt::Display for UnknownRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown model role: {}", self.0)
    }
}
impl std::error::Error for UnknownRole {}

impl FromStr for ModelRole {
    type Err = UnknownRole;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "fast" => Ok(Self::Fast),
            "deep" => Ok(Self::Deep),
            "plan" => Ok(Self::Plan),
            "review" => Ok(Self::Review),
            "subagent" => Ok(Self::Subagent),
            "vision" => Ok(Self::Vision),
            "summarizer" => Ok(Self::Summarizer),
            _ => Err(UnknownRole(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelection {
    pub provider: String,
    pub model: String,
    pub role: ModelRole,
}

impl ModelSelection {
    pub fn new(provider: impl Into<String>, model: impl Into<String>, role: ModelRole) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            role,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCatalogError {
    UnknownRole(UnknownRole),
    UnknownModel(String),
    EmptyProvider,
    EmptyModel,
}

impl fmt::Display for ModelCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRole(e) => e.fmt(f),
            Self::UnknownModel(m) => write!(f, "unknown model: {m}"),
            Self::EmptyProvider => f.write_str("provider cannot be empty"),
            Self::EmptyModel => f.write_str("model cannot be empty"),
        }
    }
}
impl std::error::Error for ModelCatalogError {}
impl From<UnknownRole> for ModelCatalogError {
    fn from(e: UnknownRole) -> Self {
        Self::UnknownRole(e)
    }
}
impl From<Infallible> for ModelCatalogError {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModelCatalog {
    entries: Vec<ModelSelection>,
    active: Option<usize>,
}

impl ModelCatalog {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, selection: ModelSelection) -> Result<(), ModelCatalogError> {
        if selection.provider.trim().is_empty() {
            return Err(ModelCatalogError::EmptyProvider);
        }
        if selection.model.trim().is_empty() {
            return Err(ModelCatalogError::EmptyModel);
        }
        if let Some(i) = self.entries.iter().position(|x| x.role == selection.role) {
            self.entries[i] = selection;
        } else {
            self.entries.push(selection);
        }
        Ok(())
    }
    pub fn select_role<R>(&mut self, role: R) -> Result<&ModelSelection, ModelCatalogError>
    where
        R: TryInto<ModelRole>,
        R::Error: Into<ModelCatalogError>,
    {
        let role = role.try_into().map_err(Into::into)?;
        let i = self
            .entries
            .iter()
            .position(|x| x.role == role)
            .ok_or_else(|| ModelCatalogError::UnknownModel(role.to_string()))?;
        self.active = Some(i);
        Ok(&self.entries[i])
    }
    pub fn active(&self) -> Option<&ModelSelection> {
        self.active.and_then(|i| self.entries.get(i))
    }
    pub fn list(&self) -> &[ModelSelection] {
        &self.entries
    }
    #[allow(dead_code)]
    pub fn validate_role<R>(&self, role: R) -> Result<ModelRole, ModelCatalogError>
    where
        R: TryInto<ModelRole>,
        R::Error: Into<ModelCatalogError>,
    {
        role.try_into().map_err(Into::into)
    }
    pub fn validate_model(&self, model: &str) -> Result<(), ModelCatalogError> {
        if self.entries.iter().any(|x| x.model == model) {
            Ok(())
        } else {
            Err(ModelCatalogError::UnknownModel(model.to_owned()))
        }
    }
}

impl TryFrom<&str> for ModelRole {
    type Error = UnknownRole;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}
impl TryFrom<String> for ModelRole {
    type Error = UnknownRole;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}
