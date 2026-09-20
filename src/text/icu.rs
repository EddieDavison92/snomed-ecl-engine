use super::TextError;
use std::ffi::{c_char, c_void};
use std::ops::Range;
use std::ptr::NonNull;

unsafe extern "C" {
    fn snomed_search_open(
        pattern: *const u16,
        length: i32,
        locale: *const c_char,
        result: *mut i32,
    ) -> *mut c_void;
    fn snomed_search_close(state: *mut c_void);
    fn snomed_search_find(
        state: *mut c_void,
        text: *const u16,
        length: i32,
        minimum: i32,
        flags: i32,
        limit: *mut i32,
        result: *mut i32,
    ) -> i32;
}

/// A query-local ICU search. It owns its pattern and cannot be shared between threads.
#[derive(Debug)]
pub struct Search(NonNull<c_void>);

impl Search {
    pub fn new(pattern: &str, language: [u8; 2]) -> Result<Self, TextError> {
        if pattern.is_empty()
            || pattern.len() > 65_536
            || !language.iter().all(u8::is_ascii_lowercase)
        {
            return Err(TextError::InvalidInput);
        }
        let pattern: Vec<_> = pattern.encode_utf16().collect();
        let locale = [language[0], language[1], 0];
        let mut status = 0;
        // The shim copies the pattern and locale-dependent ICU state before returning.
        let pointer = unsafe {
            snomed_search_open(
                pattern.as_ptr(),
                pattern.len() as i32,
                locale.as_ptr().cast(),
                &mut status,
            )
        };
        NonNull::new(pointer)
            .map(Self)
            .ok_or(TextError::Icu(status))
    }

    /// `flags`: 1 anchors the start, 2 anchors the end, 4 requires a word start.
    pub fn find(
        &mut self,
        text: &[u16],
        minimum: usize,
        flags: u8,
    ) -> Result<Option<Range<usize>>, TextError> {
        let length = i32::try_from(text.len()).map_err(|_| TextError::InvalidInput)?;
        if minimum > text.len() || flags > 7 {
            return Err(TextError::InvalidInput);
        }
        if text.is_empty() {
            return Ok(None);
        }
        let mut status = 0;
        let mut end = 0;
        // Both outputs are writable. The shim resets its text references before returning.
        let start = unsafe {
            snomed_search_find(
                self.0.as_ptr(),
                text.as_ptr(),
                length,
                minimum as i32,
                i32::from(flags),
                &mut end,
                &mut status,
            )
        };
        if status > 0 {
            return Err(TextError::Icu(status));
        }
        if start == -1 {
            return Ok(None);
        }
        if start < 0 || start < minimum as i32 || end < start || end > length {
            return Err(TextError::InvalidInput);
        }
        Ok(Some(start as usize..end as usize))
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        // This handle is uniquely owned and is closed exactly once.
        unsafe { snomed_search_close(self.0.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn asymmetric_collation_and_locale_tailoring_match_ecl_examples() {
        for (language, pattern, target, expected) in [
            (*b"en", "resume", "RÉSUMÉ", true),
            (*b"en", "résumé", "resume", false),
            (*b"en", "réSume", "Résumé", true),
            (*b"en", "sjogren", "sjøgren", true),
            (*b"en", "sjögren", "sjøgren", false),
            (*b"sv", "sjogren", "sjögren", false),
            (*b"sv", "sjögren", "sjøgren", true),
            (*b"da", "Aalborg", "Ålborg", true),
            (*b"en", "résumé", "re\u{301}sume\u{301}", true),
        ] {
            let mut search = Search::new(pattern, language).unwrap();
            assert_eq!(
                search
                    .find(&target.encode_utf16().collect::<Vec<_>>(), 0, 3)
                    .unwrap()
                    .is_some(),
                expected,
                "{pattern} / {target}"
            );
        }
    }
    #[test]
    fn boundaries_anchors_and_repeated_borrowed_targets() {
        let mut search = Search::new("gas", *b"en").unwrap();
        assert!(search
            .find(&"gastric".encode_utf16().collect::<Vec<_>>(), 0, 4)
            .unwrap()
            .is_some());
        assert!(search
            .find(&"intr agastric".encode_utf16().collect::<Vec<_>>(), 0, 4)
            .unwrap()
            .is_none());
        assert!(search
            .find(&"gas gas".encode_utf16().collect::<Vec<_>>(), 0, 2)
            .unwrap()
            .is_some());
        assert!(search
            .find(&"gastric".encode_utf16().collect::<Vec<_>>(), 0, 2)
            .unwrap()
            .is_none());
        assert_eq!(search.find(&[], 0, 0).unwrap(), None);
        assert_eq!(search.find(&[0x61], 2, 0), Err(TextError::InvalidInput));
    }
}
