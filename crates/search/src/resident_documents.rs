// Copyright 2026 Nowledge
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Immutable resident document pages shared by projection read generations.

use crate::SearchDocument;
use hawdb_storage::cow::{CowPageWeight, CowSegmentedMap};
use std::collections::BTreeMap;
use std::sync::Arc;

pub(crate) trait DocumentView {
    fn len(&self) -> usize;
    fn get(&self, id: &str) -> Option<&SearchDocument>;
    fn iter(&self) -> impl DoubleEndedIterator<Item = (&String, &SearchDocument)> + Clone;
    fn range_documents<'a>(
        &'a self,
        first: &'a str,
        last: &'a str,
    ) -> impl Iterator<Item = (&'a String, &'a SearchDocument)>;

    fn values(&self) -> impl DoubleEndedIterator<Item = &SearchDocument> + Clone {
        self.iter().map(|(_, document)| document)
    }

    fn keys(&self) -> impl DoubleEndedIterator<Item = &String> + Clone {
        self.iter().map(|(id, _)| id)
    }
}

impl DocumentView for BTreeMap<String, SearchDocument> {
    fn len(&self) -> usize {
        self.len()
    }

    fn get(&self, id: &str) -> Option<&SearchDocument> {
        self.get(id)
    }

    fn iter(&self) -> impl DoubleEndedIterator<Item = (&String, &SearchDocument)> + Clone {
        self.iter()
    }

    fn range_documents<'a>(
        &'a self,
        first: &'a str,
        last: &'a str,
    ) -> impl Iterator<Item = (&'a String, &'a SearchDocument)> {
        self.range::<str, _>((
            std::ops::Bound::Included(first),
            std::ops::Bound::Included(last),
        ))
    }
}

#[derive(Debug, Clone)]
struct SharedDocument {
    document: Arc<SearchDocument>,
    retained_bytes: usize,
    key_capacity: usize,
}

impl CowPageWeight for SharedDocument {
    fn cow_page_bytes(&self) -> usize {
        self.retained_bytes
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ResidentDocuments {
    pages: CowSegmentedMap<String, SharedDocument>,
    // Keep the sum wide so removing a value recovers from a saturated bound.
    retained_bytes: u128,
}

impl ResidentDocuments {
    pub(crate) fn shares_storage_with(&self, other: &Self) -> bool {
        self.pages.shares_storage_with(&other.pages)
    }

    pub(crate) fn len(&self) -> usize {
        self.pages.len()
    }
    pub(crate) fn values(&self) -> impl DoubleEndedIterator<Item = &SearchDocument> + Clone {
        DocumentView::values(self)
    }
    #[cfg(test)]
    pub(crate) fn keys(&self) -> impl DoubleEndedIterator<Item = &String> + Clone {
        DocumentView::keys(self)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub(crate) fn contains_key(&self, id: &str) -> bool {
        self.pages.contains_key(id)
    }

    pub(crate) fn insert(&mut self, id: String, document: SearchDocument) {
        // Capacity, not serialized length: include document buffers, B-tree
        // nodes, page-directory entries and Arc metadata. This is a conservative
        // owner bound; shared generations can charge the same storage twice.
        let key_capacity = self
            .pages
            .get(id.as_str())
            .map_or(id.capacity(), |previous| {
                previous.key_capacity.max(id.capacity())
            });
        let retained_bytes = crate::build_memory::document_bytes(&document)
            .unwrap_or(usize::MAX)
            .saturating_add(key_capacity)
            .saturating_add(crate::build_memory::MAP_ENTRY_BYTES);
        self.retained_bytes += retained_bytes as u128;
        let previous = self.pages.insert(
            id,
            SharedDocument {
                document: Arc::new(document),
                retained_bytes,
                key_capacity,
            },
        );
        if let Some(previous) = previous {
            self.retained_bytes -= previous.retained_bytes as u128;
        }
    }

    pub(crate) fn remove(&mut self, id: &str) -> Option<Arc<SearchDocument>> {
        let previous = self.pages.remove(id)?;
        self.retained_bytes -= previous.retained_bytes as u128;
        Some(previous.document)
    }

    pub(crate) fn retained_capacity_bytes(&self) -> usize {
        usize::try_from(self.retained_bytes)
            .unwrap_or(usize::MAX)
            .saturating_add(crate::build_memory::ACCOUNT_METADATA_BYTES)
            .saturating_add(self.pages.page_directory_capacity_bytes())
    }
}

impl From<BTreeMap<String, SearchDocument>> for ResidentDocuments {
    fn from(documents: BTreeMap<String, SearchDocument>) -> Self {
        let mut resident = Self::default();
        for (id, document) in documents {
            resident.insert(id, document);
        }
        resident
    }
}

impl DocumentView for ResidentDocuments {
    fn len(&self) -> usize {
        self.pages.len()
    }

    fn get(&self, id: &str) -> Option<&SearchDocument> {
        self.pages.get(id).map(|entry| entry.document.as_ref())
    }

    fn iter(&self) -> impl DoubleEndedIterator<Item = (&String, &SearchDocument)> + Clone {
        self.pages
            .iter()
            .map(|(id, entry)| (id, entry.document.as_ref()))
    }

    fn range_documents<'a>(
        &'a self,
        first: &'a str,
        last: &'a str,
    ) -> impl Iterator<Item = (&'a String, &'a SearchDocument)> {
        self.pages
            .range(first, last)
            .map(|(id, entry)| (id, entry.document.as_ref()))
    }
}

#[cfg(test)]
impl std::ops::Index<&str> for ResidentDocuments {
    type Output = SearchDocument;

    fn index(&self, id: &str) -> &Self::Output {
        self.get(id).expect("indexed resident document exists")
    }
}

#[cfg(test)]
impl PartialEq for ResidentDocuments {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(id: &str, content: &str) -> SearchDocument {
        SearchDocument {
            id: id.into(),
            title: id.into(),
            content: content.into(),
            embedding: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn snapshots_preserve_ordered_ranges_and_release_deleted_bodies() {
        let mut current = ResidentDocuments::default();
        for i in 0..1025 {
            let id = format!("document-{i:04}");
            current.insert(id.clone(), document(&id, &"x".repeat(4096)));
        }
        let old = current.clone();
        assert!(current.pages.shares_storage_with(&old.pages));
        let retired = Arc::downgrade(&current.pages.get("document-0000").unwrap().document);
        let untouched = Arc::downgrade(&current.pages.get("document-1024").unwrap().document);
        current.insert("document-0000".into(), document("document-0000", "updated"));
        current.remove("document-0001").unwrap();
        assert_eq!(old.len(), 1025);
        assert_eq!(current.len(), 1024);
        assert_eq!(old.get("document-0000").unwrap().content.len(), 4096);
        assert_eq!(current.get("document-0000").unwrap().content, "updated");
        assert!(Arc::ptr_eq(
            &untouched.upgrade().unwrap(),
            &current.pages.get("document-1024").unwrap().document
        ));
        assert_eq!(
            old.range_documents("document-0000", "document-0002")
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            ["document-0000", "document-0001", "document-0002"]
        );
        assert_eq!(
            current
                .range_documents("document-0000", "document-0002")
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            ["document-0000", "document-0002"]
        );
        assert_eq!(
            current.keys().next_back().map(String::as_str),
            Some("document-1024")
        );
        assert!(retired.upgrade().is_some());
        drop(old);
        assert!(retired.upgrade().is_none());
    }

    #[test]
    fn retained_bound_covers_replaced_keys_and_empty_directory_capacity() {
        let mut documents = ResidentDocuments::default();
        let mut large_key = String::with_capacity(1 << 20);
        large_key.push_str("key");
        documents.insert(large_key, document("key", "first"));
        documents.insert("key".into(), document("key", "next"));
        assert!(documents.retained_capacity_bytes() >= 1 << 20);
        for i in 0..2048 {
            let id = format!("{i:04}");
            documents.insert(id.clone(), document(&id, "value"));
        }
        let ids = documents.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            documents.remove(&id).unwrap();
        }
        assert!(documents.is_empty());
        assert!(
            documents.retained_capacity_bytes() >= documents.pages.page_directory_capacity_bytes()
        );
        assert!(documents.retained_capacity_bytes() < 1 << 20);
    }
}
