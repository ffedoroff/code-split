//! Shared, language-agnostic layer for Code Split plugins: the complexity
//! metrics engine, the file-graph finalize pass, and timing/logging helpers.
//! Language plugins (`code-split-plugin-rust`, `-python`, `-javascript`) build
//! on top of this; it knows nothing about any specific language.

pub mod complexity;
pub mod finalize;
pub mod logger;
