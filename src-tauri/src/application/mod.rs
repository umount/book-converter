//! Domain application services shared by commands and assistant tools.
pub mod book;
mod book_budget;
pub mod book_delete;
pub mod book_edit;
pub mod book_export;
pub mod book_glossary;
mod book_json;
mod book_language;
pub mod book_metadata;
pub mod book_reference;
mod book_terms;
pub mod preferences;
pub mod runtime;

#[cfg(test)]
mod book_tests;

pub mod book_presentation;

pub mod assistant;

pub mod profiles;

pub mod book_search;

pub mod book_retarget;

#[cfg(test)]
mod book_workflow_tests;

pub mod book_volume;
