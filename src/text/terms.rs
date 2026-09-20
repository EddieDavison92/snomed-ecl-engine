use super::{Search, TextError};
use crate::ecl::{SearchTerm, MAX_NODES, MAX_QUERY_BYTES};
use std::collections::BTreeMap;

pub struct Terms<'a> {
    source: &'a [SearchTerm],
    languages: BTreeMap<[u8; 2], Vec<Vec<Option<Search>>>>,
    buffer: Vec<u16>,
}

impl<'a> Terms<'a> {
    pub fn new(source: &'a [SearchTerm]) -> Result<Self, TextError> {
        let mut nodes = 0usize;
        let mut bytes = 0usize;
        for term in source {
            nodes = nodes.saturating_add(1);
            match term {
                SearchTerm::Match(words) => {
                    nodes = nodes.saturating_add(words.len());
                    for word in words {
                        bytes = bytes.saturating_add(word.len());
                    }
                }
                SearchTerm::Wild(parts) => {
                    nodes = nodes.saturating_add(parts.len());
                    for text in parts.iter().flatten() {
                        bytes = bytes.saturating_add(text.len());
                    }
                    if parts.windows(2).any(|w| w[0].is_some() && w[1].is_some()) {
                        return Err(TextError::InvalidInput);
                    }
                }
            }
        }
        if source.is_empty()
            || nodes > MAX_NODES
            || bytes > MAX_QUERY_BYTES
            || source.iter().any(|term| match term {
                SearchTerm::Match(words) => words.is_empty() || words.iter().any(String::is_empty),
                SearchTerm::Wild(parts) => {
                    parts.is_empty() || parts.iter().flatten().any(String::is_empty)
                }
            })
        {
            return Err(TextError::InvalidInput);
        }
        Ok(Self {
            source,
            languages: BTreeMap::new(),
            buffer: Vec::new(),
        })
    }

    pub fn work(&self, target: &str) -> usize {
        self.work_bytes(target.len())
    }

    pub fn work_bytes(&self, target_bytes: usize) -> usize {
        self.source
            .iter()
            .map(|term| match term {
                SearchTerm::Match(words) => words
                    .iter()
                    .map(|p| p.len().saturating_add(target_bytes).saturating_add(1))
                    .sum::<usize>(),
                SearchTerm::Wild(parts) => parts
                    .iter()
                    .flatten()
                    .map(|p| p.len().saturating_add(target_bytes).saturating_add(1))
                    .sum::<usize>()
                    .saturating_add(1),
            })
            .fold(0, usize::saturating_add)
    }

    pub fn matches(&mut self, target: &str, language: [u8; 2]) -> Result<bool, TextError> {
        if !self.languages.contains_key(&language) {
            let mut compiled = Vec::with_capacity(self.source.len());
            for term in self.source {
                compiled.push(match term {
                    SearchTerm::Match(words) => words.iter().map(|_| None).collect(),
                    SearchTerm::Wild(parts) => parts.iter().flatten().map(|_| None).collect(),
                });
            }
            self.languages.insert(language, compiled);
        }
        self.buffer.clear();
        self.buffer.extend(target.encode_utf16());
        let compiled = self.languages.get_mut(&language).unwrap();
        for (term, searches) in self.source.iter().zip(compiled) {
            let matched = match term {
                SearchTerm::Match(words) => {
                    let mut matched = true;
                    for (word, search) in words.iter().zip(searches) {
                        let search = compile(search, word, language)?;
                        if search.find(&self.buffer, 0, 4)?.is_none() {
                            matched = false;
                            break;
                        }
                    }
                    matched
                }
                SearchTerm::Wild(parts) => {
                    let mut end = 0;
                    let mut matched = true;
                    let mut searches = searches.iter_mut();
                    for (i, part) in parts.iter().enumerate() {
                        let Some(text) = part else {
                            continue;
                        };
                        let flags = u8::from(i == 0) | (u8::from(i + 1 == parts.len()) << 1);
                        let search = compile(searches.next().unwrap(), text, language)?;
                        let Some(found) = search.find(&self.buffer, end, flags)? else {
                            matched = false;
                            break;
                        };
                        end = found.end;
                    }
                    matched
                }
            };
            if matched {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn compile<'a>(
    slot: &'a mut Option<Search>,
    pattern: &str,
    language: [u8; 2],
) -> Result<&'a mut Search, TextError> {
    if slot.is_none() {
        *slot = Some(Search::new(pattern, language)?);
    }
    Ok(slot.as_mut().unwrap())
}
