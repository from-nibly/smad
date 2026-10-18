//! GUI-free browser domain model and reducers.

pub mod comparison;
pub mod document_uri;
pub mod git_source;
pub mod labels;
pub mod markdown;
pub mod model;
pub mod persistence;
pub mod reconcile;
pub mod snapshots;
pub mod source;

pub use model::{BrowserModel, Document, DocumentKey, DocumentLifecycle, OpenOutcome, Project};
pub use smad_protocol::ProjectKey;

/// Persistence implementations consume and restore the pure model through this seam.
pub trait BrowserStateStore {
    type Error;

    fn save(&self, model: &BrowserModel) -> Result<(), Self::Error>;
    fn load(&self) -> Result<BrowserModel, Self::Error>;
}
