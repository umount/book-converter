//! Manga-specific automatic processing adapters, independent of book operations.
pub mod recognition;
pub mod regions;
pub mod pipeline;
pub mod runtime;

#[cfg(test)]
mod tests;
pub mod view;

pub mod preflight;

pub mod translation;
pub mod translation_pipeline;

pub mod local;
pub mod image_pipeline;

pub mod automatic;

pub mod rebuild;
