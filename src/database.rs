#![allow(unused)]

use std::path::Path;
use std::usize;

use rayon::prelude::*;
use rayon_hash::HashMap;
use rayon_hash::HashSet;
use whatlang::Lang;

use helpers::canonicalize;
use storage::Storage;

#[derive(Debug)]
pub struct Database {
    storage: Storage,
    urls: HashMap<u64, String>,
    by_language: HashMap<Lang, HashSet<u64>>,
    by_word: HashMap<String, HashSet<u64>>,
    by_title_word: HashMap<String, HashSet<u64>>,
}

pub struct Page {
    pub lang: Lang,
    pub content: String,
}

impl Database {
    pub fn new(storage: Storage) -> Database {
        Database {
            storage,
            urls: Default::default(),
            by_language: Default::default(),
            by_word: Default::default(),
            by_title_word: Default::default(),
        }
    }

    pub fn len(&self) -> u64 {
        self.storage.get_num_pages()
    }

    fn index_words(&mut self, content: &str) -> Option<u64> {
        let mut debug = false;
        let title_end = content.find('\n').unwrap_or(0);
        let (mut title, content) = content.split_at(title_end);

        if title.len() > 250 {
            title = ""; // title is invalid
        }

        let mut title_words = title
            .split_whitespace()
            .filter_map(canonicalize)
            .collect::<Vec<_>>();

        title_words.sort_unstable();
        title_words.dedup();

        let mut words = content
            .split_whitespace()
            .filter_map(canonicalize)
            .collect::<Vec<_>>();

        words.sort_unstable();
        words.dedup();

        if words.len() < 10 {
            return None;
        }

        let url = self.storage.next_id();

        for title_word in title_words {
            self.by_title_word
                .entry(title_word)
                .or_insert_with(HashSet::new)
                .insert(url);
        }

        for word in words {
            self.by_word
                .entry(word)
                .or_insert_with(HashSet::new)
                .insert(url);
        }

        Some(url)
    }

    pub fn insert(&mut self, url: String, page: Page) {
        let Page { content, lang } = page;

        // if the page doesn't contain at least 10 words,
        // then we don't care about it.
        if let Some(uid) = self.index_words(&content) {
            self.urls.insert(uid, url);
            self.by_language
                .entry(lang)
                .or_insert_with(HashSet::new)
                .insert(uid);
        }
    }

    pub fn shrink(&mut self) {
        // now, URLs are being retrieved from on-disk indexes
        self.urls = HashMap::new();

        self.by_language.shrink_to_fit();
        self.by_word.shrink_to_fit();

        self.by_language
            .iter_mut()
            .for_each(|(_lang, hashset)| hashset.shrink_to_fit());

        self.by_word
            .iter_mut()
            .for_each(|(_lang, hashset)| hashset.shrink_to_fit());
    }

    pub fn persist(&mut self) {
        self.storage.store_urls(&self.urls);
    }

    pub fn query(&mut self, words: Vec<String>, lang: Option<Lang>) {
        let mut sets = words
            .into_iter()
            .filter_map(|word| canonicalize(&word))
            .filter_map(|word| self.by_word.get(&word))
            .map(|x| x.to_owned())
            .collect::<Vec<_>>();

        if sets.is_empty() {
            println!("no matches found");
            return;
        }

        if let Some(lang) = lang {
            if let Some(lang_set) = self.by_language.get(&lang) {
                sets.push(lang_set.to_owned());
            }
        }

        let mut iter = sets.into_iter();
        let set = iter.next().unwrap();
        let results = iter.fold(set, |set1, set2| &set1 & &set2)
            .iter()
            .cloned()
            .collect::<Vec<_>>();

        let len = results.len();

        let results = self.storage
            .get_urls(results.into_iter().take(10).collect());

        println!("{} results", len);

        results.iter().for_each(|url| println!("{}", url));
    }
}
