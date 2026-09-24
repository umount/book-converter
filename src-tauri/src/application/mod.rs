//! Domain application services shared by commands and assistant tools.
pub mod book;
pub mod book_glossary;
pub mod preferences;
pub mod book_edit;
pub mod book_export;
pub mod book_reference;
pub mod book_metadata;
pub mod runtime;

#[cfg(test)]
mod book_tests;

pub mod book_presentation;
