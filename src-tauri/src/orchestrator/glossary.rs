//! Per-run glossary cache, learning and incremental persistence.

use std::collections::HashSet;

use anyhow::Result;

use crate::config::Config;
use crate::glossary::{self, Term};
use crate::state::Store;
use crate::translator::Translate;

pub(super) struct GlossarySession {
    terms: Vec<Term>,
    dirty: HashSet<String>,
}

impl GlossarySession {
    pub(super) fn load(store: &Store) -> Result<Self> {
        Ok(Self {
            terms: store.load_glossary()?,
            dirty: HashSet::new(),
        })
    }

    pub(super) fn relevant<'a>(&'a self, source: &str) -> Vec<&'a Term> {
        glossary::relevant_terms(&self.terms, source)
    }

    pub(super) async fn learn<C: Translate>(
        &mut self,
        client: &C,
        config: &Config,
        store: &Store,
        source: &str,
        translation: &str,
    ) -> Result<()> {
        let new_terms =
            crate::translator::extract_terms(client, config, source, translation, 2).await?;
        for term in &new_terms {
            self.dirty.insert(term.source.clone());
        }
        glossary::merge(&mut self.terms, new_terms);
        self.flush(store)?;
        Ok(())
    }

    /// Write only terms touched in this run. A concurrent manual edit keeps its
    /// canonical rendering; this run contributes only the higher frequency.
    pub(super) fn flush(&mut self, store: &Store) -> Result<usize> {
        if self.dirty.is_empty() {
            return Ok(0);
        }
        let mut rows = Vec::with_capacity(self.dirty.len());
        for term in self
            .terms
            .iter()
            .filter(|term| self.dirty.contains(&term.source))
        {
            match store.term(&term.source)? {
                Some(stored) => rows.push(Term {
                    frequency: term.frequency.max(stored.frequency),
                    ..stored
                }),
                None => rows.push(term.clone()),
            }
        }
        let references: Vec<&Term> = rows.iter().collect();
        store.upsert_terms(&references)?;
        self.dirty.clear();
        tracing::info!(terms = rows.len(), "glossary flushed at end of run");
        Ok(rows.len())
    }
}
