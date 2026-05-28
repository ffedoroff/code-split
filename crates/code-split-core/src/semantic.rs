use crate::builder::GraphBuilder;
use std::path::Path;

/// Pluggable semantic backend. The single permitted entry point through
/// which a name-resolving analyzer (e.g. `code-split-sema` backed by `ra_ap_*`)
/// contributes nodes and edges to a `GraphBuilder`.
pub trait SemanticIndex {
    type Error: std::error::Error + Send + Sync + 'static;
    fn analyze(&self, workspace: &Path, builder: &mut GraphBuilder) -> Result<(), Self::Error>;
}
