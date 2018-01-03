use helpers::add_pairs;
use helpers::canonicalize;
use std::fs::File;
use std::fs::read_dir;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::time::Instant;

use flate2::read::MultiGzDecoder;
use rayon::prelude::*;
use whatlang::{detect, Lang};

use commoncrawl::{GetWetRef, WetRef};
use database::Database;
use database::Page;
use errors::StrError;
use helpers::ReadableDuration;

fn load_source(source: PathBuf) -> Result<Vec<(String, Page)>, StrError> {
    let mut raw_pages = Vec::new();

    // shorten peak memory usage time by deallocating `content` after this block
    {
        let is_gzip = source.to_str().unwrap().ends_with(".gz");

        let mut file = File::open(source)?;
        let content = &mut Vec::new();

        if is_gzip {
            MultiGzDecoder::new(BufReader::new(file)).read_to_end(content)?;
        } else {
            file.read_to_end(content)?;
        }

        content.shrink_to_fit();

        let mut remaining: &[u8] = content;
        while !remaining.is_empty() {
            let (blob, rem) = remaining.next_wet_ref();
            remaining = rem;
            if let WetRef::Conversion { url, content, .. } = blob {
                raw_pages.push((url.to_string(), content.to_string()))
            }
        }
    }

    raw_pages.shrink_to_fit();

    let mut pages = raw_pages
        .into_par_iter()
        .filter_map(|(url, content)| {
            let lang = detect(&content)?.lang();
            let title_end = content.find('\n').unwrap_or(0);
            let (mut title, content) = content.split_at(title_end);

            if title.len() > 280 {
                title = ""; // title is invalid
            }

            let mut title = title
                .split_whitespace()
                .filter_map(canonicalize)
                .collect::<Vec<_>>();

            add_pairs(&mut title);

            title.sort_unstable();
            title.dedup();
            title.shrink_to_fit();

            let mut words = content
                .split_whitespace()
                .filter_map(canonicalize)
                .collect::<Vec<_>>();

            if words.len() < 10 {
                return None;
            }

            add_pairs(&mut words);

            words.sort_unstable();
            words.dedup();
            words.shrink_to_fit();

            if lang == Lang::Eng || lang == Lang::Spa || lang == Lang::Fra {
                Some((url, Page { lang, title, words }))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    pages.shrink_to_fit();

    Ok(pages)
}

fn path_to_files(path: String) -> Vec<PathBuf> {
    let path = PathBuf::from(path);
    if path.is_file() {
        return vec![path];
    }

    let mut files = Vec::new();
    if let Ok(dir) = read_dir(&path) {
        for entry in dir {
            if let Ok(entry) = entry {
                let entry = entry.path();
                let file_name = entry.to_str().unwrap();
                if entry.is_file() && (file_name.ends_with(".wet") || entry.ends_with(".wet.gz")) {
                    files.push(entry.to_owned());
                }
            }
        }
    } else {
        panic!("ERROR: invalid path provided! Path was {}", path.display());
    }

    files
}

impl Database {
    pub fn import(&mut self, sources: Vec<String>) -> Result<(), StrError> {
        let starting_page_count = self.len();

        let now = Instant::now();

        println!("loading sources");
        let results = sources
            .into_par_iter()
            .flat_map(path_to_files)
            .into_par_iter()
            .map(load_source)
            .collect::<Vec<_>>();

        println!("sources loaded, now importing into database");

        for result in results {
            let pages = result?;
            for (url, page) in pages {
                self.insert(url, page)
            }
        }

        println!("persisting database");
        self.persist();

        let elapsed = now.elapsed().readable();

        println!(
            "{} pages imported in {}",
            self.len() - starting_page_count,
            elapsed
        );

        Ok(())
    }
}
