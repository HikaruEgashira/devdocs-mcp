pub mod docs;
pub use docs::DocRouter;
pub mod devdocs;
pub mod npm;
pub mod pypi;
pub mod golang;
#[cfg(test)]
mod tests;