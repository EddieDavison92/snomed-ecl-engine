//! Word index over description terms, for finding concepts by name.
//!
//! ECL's `{{ term = "..." }}` filter scans the descriptions of everything in
//! scope, which costs over a second on a large hierarchy and exceeds the work
//! limit on the whole edition. A browser needs an answer while the user is
//! still typing, so words are extracted once at build time and searched here
//! by binary search and list intersection.
//!
//! Normalisation happens at build time, so querying needs no collation library
//! and works in the build without ICU.
use super::*;
use std::sync::OnceLock;

/// Postings as plain u32s.
const MAGIC_V1: &[u8; 8] = b"SNECLSR1";
/// Postings as varint deltas, a third of the size; decoded to u32s on load.
const MAGIC: &[u8; 8] = b"SNECLSR2";

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SearchManifest {
    pub bytes: u64,
    pub sha256: String,
    pub words: usize,
    pub postings: usize,
}

/// Folds one character towards lowercase ASCII, so a search typed without
/// accents still finds a term that carries them. Anything still not
/// alphanumeric after folding ends a word.
fn fold(ch: char) -> char {
    match ch {
        '\u{00e0}'..='\u{00e5}' | '\u{00c0}'..='\u{00c5}' | '\u{00e6}' | '\u{00c6}' => 'a',
        '\u{00e7}' | '\u{00c7}' => 'c',
        '\u{00e8}'..='\u{00eb}' | '\u{00c8}'..='\u{00cb}' => 'e',
        '\u{00ec}'..='\u{00ef}' | '\u{00cc}'..='\u{00cf}' => 'i',
        '\u{00f1}' | '\u{00d1}' => 'n',
        '\u{00f2}'..='\u{00f6}' | '\u{00d2}'..='\u{00d6}' | '\u{00f8}' | '\u{00d8}' => 'o',
        '\u{00f9}'..='\u{00fc}' | '\u{00d9}'..='\u{00dc}' => 'u',
        '\u{00fd}' | '\u{00ff}' | '\u{00dd}' => 'y',
        '\u{00df}' => 's',
        _ => ch.to_ascii_lowercase(),
    }
}

/// Splits text into normalised words. "Type 2 diabetes mellitus" gives four.
pub fn words(text: &str, out: &mut Vec<String>) {
    let mut word = String::new();
    for ch in text.chars() {
        let ch = fold(ch);
        if ch.is_ascii_alphanumeric() {
            word.push(ch);
        } else if !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
}

/// Every (word, concept) pair in an edition's active descriptions.
///
/// Inactive descriptions are skipped: they are retired wordings, and matching
/// them surfaces concepts under names the release no longer publishes.
pub fn search_pairs(index: &DescriptionIndex, concepts: usize) -> Result<Vec<(String, u32)>> {
    let mut pairs = Vec::new();
    let mut buffer = Vec::new();
    for concept in 0..concepts as u32 {
        for row in index.for_concept(concept) {
            if !index.active(row) {
                continue;
            }
            buffer.clear();
            index.with_term(row, |term| words(term, &mut buffer))?;
            for word in buffer.drain(..) {
                pairs.push((word, concept));
            }
        }
    }
    Ok(pairs)
}

#[derive(Debug, Default)]
pub struct SearchIndex {
    /// Sorted unique words, concatenated.
    text: Vec<u8>,
    /// Where each word starts in `text`. One longer than the word count.
    text_offsets: Vec<u32>,
    /// Where each word's concept list starts in `postings`.
    posting_offsets: Vec<u32>,
    /// Concept ordinals, sorted and deduplicated within each word.
    postings: Vec<u32>,
}

impl SearchIndex {
    pub fn word_count(&self) -> usize {
        self.text_offsets.len().saturating_sub(1)
    }
    pub fn posting_count(&self) -> usize {
        self.postings.len()
    }
    fn word(&self, index: usize) -> &[u8] {
        &self.text[self.text_offsets[index] as usize..self.text_offsets[index + 1] as usize]
    }
    fn concepts(&self, index: usize) -> &[u32] {
        &self.postings
            [self.posting_offsets[index] as usize..self.posting_offsets[index + 1] as usize]
    }

    /// Builds from (word, concept ordinal) pairs. Duplicates are fine.
    pub fn build(mut pairs: Vec<(String, u32)>) -> Result<Self> {
        pairs.sort_unstable();
        pairs.dedup();
        let mut index = Self {
            text_offsets: vec![0],
            posting_offsets: vec![0],
            ..Self::default()
        };
        let mut current: Option<String> = None;
        for (word, ordinal) in pairs {
            if current.as_deref() != Some(word.as_str()) {
                index.text.extend_from_slice(word.as_bytes());
                index
                    .text_offsets
                    .push(u32::try_from(index.text.len()).context("Search text exceeds u32")?);
                index.posting_offsets.push(
                    u32::try_from(index.postings.len()).context("Search postings exceed u32")?,
                );
                current = Some(word);
            }
            index.postings.push(ordinal);
            *index.posting_offsets.last_mut().expect("seeded") =
                u32::try_from(index.postings.len()).context("Search postings exceed u32")?;
        }
        index.validate()?;
        Ok(index)
    }

    fn validate(&self) -> Result<()> {
        let n = self.word_count();
        ensure!(
            self.posting_offsets.len() == n + 1
                && self.text_offsets.first() == Some(&0)
                && self.posting_offsets.first() == Some(&0),
            "Invalid search index offsets"
        );
        ensure!(
            self.text_offsets.last().copied() == u32::try_from(self.text.len()).ok()
                && self.posting_offsets.last().copied() == u32::try_from(self.postings.len()).ok(),
            "Search index offsets do not cover their arrays"
        );
        ensure!(
            self.text_offsets.windows(2).all(|w| w[0] <= w[1])
                && self.posting_offsets.windows(2).all(|w| w[0] <= w[1]),
            "Non-monotonic search index offsets"
        );
        Ok(())
    }

    /// Checks that words are sorted, unique and valid text, and that every
    /// posting list is sorted. Only `verify` pays for this.
    pub fn validate_order(&self) -> Result<()> {
        for i in 0..self.word_count() {
            ensure!(
                std::str::from_utf8(self.word(i)).is_ok(),
                "Search word is not UTF-8"
            );
            ensure!(!self.word(i).is_empty(), "Empty search word");
            if i > 0 {
                ensure!(self.word(i - 1) < self.word(i), "Unsorted search words");
            }
            ensure!(
                self.concepts(i).windows(2).all(|w| w[0] < w[1]),
                "Unsorted search postings"
            );
        }
        Ok(())
    }

    /// Concepts matching every word in `query`. The last word matches as a
    /// prefix, so results narrow while the user is still typing it.
    pub fn matches(&self, query: &str) -> Vec<u32> {
        let mut terms = Vec::new();
        words(query, &mut terms);
        let Some((last, rest)) = terms.split_last() else {
            return Vec::new();
        };
        let mut result: Option<Vec<u32>> = None;
        for word in rest {
            let exact = self.exact(word.as_bytes());
            result = Some(match result {
                None => exact,
                Some(current) => intersect(&current, &exact),
            });
            if result.as_ref().is_some_and(|r| r.is_empty()) {
                return Vec::new();
            }
        }
        let prefixed = self.prefixed(last.as_bytes());
        match result {
            None => prefixed,
            Some(current) => intersect(&current, &prefixed),
        }
    }

    fn exact(&self, word: &[u8]) -> Vec<u32> {
        let n = self.word_count();
        let at = partition(n, |i| self.word(i) < word);
        if at < n && self.word(at) == word {
            self.concepts(at).to_vec()
        } else {
            Vec::new()
        }
    }

    /// Every concept under any word starting with `prefix`, sorted and unique.
    fn prefixed(&self, prefix: &[u8]) -> Vec<u32> {
        let n = self.word_count();
        let start = partition(n, |i| self.word(i) < prefix);
        let mut out = Vec::new();
        for i in start..n {
            if !self.word(i).starts_with(prefix) {
                break;
            }
            out.extend_from_slice(self.concepts(i));
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn write(&self, path: &Path) -> Result<SearchManifest> {
        let mut out = BufWriter::new(File::create_new(path)?);
        out.write_all(MAGIC)?;
        put_u32s(&mut out, &self.text_offsets)?;
        put_u64(&mut out, self.text.len() as u64)?;
        out.write_all(&self.text)?;
        put_u32s(&mut out, &self.posting_offsets)?;
        let postings = super::varint::encode(&self.posting_offsets, &self.postings)?;
        put_u64(&mut out, postings.len() as u64)?;
        out.write_all(&postings)?;
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(SearchManifest {
            bytes: path.metadata()?.len(),
            sha256: sha256(path)?,
            words: self.word_count(),
            postings: self.posting_count(),
        })
    }

    /// Opens the section, refusing postings outside `concepts` ordinals.
    pub(super) fn open(
        section: &Section,
        manifest: &SearchManifest,
        concepts: usize,
    ) -> Result<Self> {
        let (mut input, version) = Input::open_versions(section, &[MAGIC_V1, MAGIC])?;
        let text_offsets = input.u32s()?;
        let text = input.bytes()?;
        let posting_offsets = input.u32s()?;
        let postings = if version == 0 {
            input.u32s()?
        } else {
            super::varint::decode(&posting_offsets, &input.bytes()?)?
        };
        ensure!(input.remaining == 0, "Trailing search bytes");
        let index = Self {
            text,
            text_offsets,
            posting_offsets,
            postings,
        };
        index.validate()?;
        ensure!(
            index.word_count() == manifest.words && index.posting_count() == manifest.postings,
            "Search index differs from manifest"
        );
        // Legacy lists are not checked for order here, so every posting is.
        ensure!(
            index.postings.iter().all(|&p| (p as usize) < concepts),
            "Search posting outside the concept table"
        );
        Ok(index)
    }
}

/// The first index in `0..n` where `predicate` stops holding. `predicate` must
/// hold for a prefix of the range and not after it.
fn partition(n: usize, predicate: impl Fn(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, n);
    while low < high {
        let mid = low + (high - low) / 2;
        if predicate(mid) {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

/// Both inputs are sorted and unique, so this walks them once.
fn intersect(left: &[u32], right: &[u32]) -> Vec<u32> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        match left[i].cmp(&right[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                out.push(left[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        words(text, &mut out);
        out
    }

    #[test]
    fn splits_and_folds_terms_into_searchable_words() {
        assert_eq!(
            split("Type 2 diabetes mellitus"),
            ["type", "2", "diabetes", "mellitus"]
        );
        // Punctuation separates; it never becomes part of a word.
        assert_eq!(
            split("COPD - chronic/obstructive"),
            ["copd", "chronic", "obstructive"]
        );
        // Accents fold, so a query typed without them still matches.
        assert_eq!(
            split("\u{00c5}str\u{00f6}m's na\u{00ef}ve"),
            ["astrom", "s", "naive"]
        );
        assert!(split("   -- ").is_empty());
    }

    fn index() -> SearchIndex {
        let terms = [
            (0, "Asthma"),
            (1, "Asthma clinic"),
            (2, "Chronic asthmatic bronchitis"),
            (3, "Diabetes mellitus"),
            (4, "Type 2 diabetes mellitus"),
        ];
        let mut pairs = Vec::new();
        for (ordinal, term) in terms {
            for word in split(term) {
                pairs.push((word, ordinal));
            }
        }
        SearchIndex::build(pairs).unwrap()
    }

    #[test]
    fn matches_every_word_with_the_last_one_as_a_prefix() {
        let index = index();
        index.validate_order().unwrap();
        // A whole word matches only what carries it.
        assert_eq!(index.matches("clinic"), [1]);
        // The last word is a prefix, so "asthma" also reaches "asthmatic".
        assert_eq!(index.matches("asthma"), [0, 1, 2]);
        assert_eq!(index.matches("asthmatic"), [2]);
        // Several words must all appear, in any order.
        assert_eq!(index.matches("mellitus diabetes"), [3, 4]);
        assert_eq!(index.matches("2 diabetes"), [4]);
        // A word that is not a prefix of anything matches nothing.
        assert!(index.matches("asthmatics").is_empty());
        assert!(index.matches("clinic diabetes").is_empty());
        assert!(index.matches("").is_empty());
    }

    #[test]
    fn survives_a_write_and_read_round_trip() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("search.bin");
        let built = index();
        let manifest = built.write(&path).unwrap();
        assert_eq!(manifest.words, built.word_count());

        let source = Section::for_test(&path, manifest.bytes, manifest.sha256.clone());
        let reopened = SearchIndex::open(&source, &manifest, 100).unwrap();
        assert_eq!(reopened.word_count(), built.word_count());
        assert_eq!(reopened.matches("asthma"), built.matches("asthma"));
        reopened.validate_order().unwrap();

        // A posting past the last concept is refused.
        assert!(SearchIndex::open(&source, &manifest, 4).is_err());
        // So is a manifest that disagrees with the bytes.
        let wrong = SearchManifest {
            words: manifest.words + 1,
            ..manifest
        };
        assert!(SearchIndex::open(&source, &wrong, 100).is_err());
    }
}

/// Opens the section on first use, like descriptions and member tables.
#[derive(Debug, Default)]
pub struct SearchStore {
    source: Option<(Section, SearchManifest, usize)>,
    loaded: OnceLock<std::result::Result<SearchIndex, String>>,
}

impl SearchStore {
    pub(super) fn lazy(
        source: &IndexSource,
        metadata: SearchManifest,
        concepts: usize,
    ) -> Result<Self> {
        Ok(Self {
            source: Some((source.section("search.bin")?, metadata, concepts)),
            loaded: OnceLock::new(),
        })
    }
    pub fn get(&self) -> Result<Option<&SearchIndex>> {
        if self.source.is_none() {
            return Ok(None);
        }
        match self.loaded.get_or_init(|| {
            let (section, manifest, concepts) = self.source.as_ref().unwrap();
            SearchIndex::open(section, manifest, *concepts).map_err(|e| e.to_string())
        }) {
            Ok(index) => Ok(Some(index)),
            Err(message) => bail!("Search index: {message}"),
        }
    }
}
