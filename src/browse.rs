//! Menu rows for browsing TRUD releases and the indexes on disk.
use crate::library;
#[cfg(feature = "download")]
use crate::presentation::bytes;
use crate::presentation::{clean, number};
use crate::workspace::Found;

#[cfg(feature = "download")]
use crate::download::{Release, ITEMS};

/// One row per known TRUD item: its name, then its title.
#[cfg(feature = "download")]
pub fn item_rows() -> Vec<String> {
    let width = ITEMS.iter().map(|item| item.name.len()).max().unwrap_or(0);
    ITEMS
        .iter()
        .map(|item| format!("{:<width$}  {}", item.name, item.description))
        .collect()
}

/// The index built from this release's archive, matched by SHA-256.
#[cfg(feature = "download")]
pub fn indexed<'a>(release: &Release, library: &'a [Found]) -> Option<&'a Found> {
    library.iter().find(|found| {
        found
            .archive_sha256
            .eq_ignore_ascii_case(&release.archive_file_sha256)
    })
}

/// One row per release, newest first: date, name, size, and the index already
/// built from it.
#[cfg(feature = "download")]
pub fn release_rows(releases: &[Release], library: &[Found]) -> Vec<String> {
    let width = releases
        .iter()
        .map(|release| clean(&release.name).chars().count())
        .max()
        .unwrap_or(0)
        .min(40);
    releases
        .iter()
        .enumerate()
        .map(|(position, release)| {
            let mut notes = Vec::new();
            if position == 0 {
                notes.push("newest".to_owned());
            }
            if let Some(found) = indexed(release, library) {
                notes.push(format!("indexed as {}", label(found)));
            }
            let row = format!(
                "{}  {:<width$}  {:>8}  {}",
                clean(&release.release_date),
                clean(&release.name),
                bytes(release.archive_file_size_bytes),
                notes.join(", ")
            );
            row.trim_end().to_owned()
        })
        .collect()
}

/// One row per index: name, edition and release date, active concepts, and
/// the distributor's release name when the index recorded one.
pub fn index_rows(found: &[Found]) -> Vec<String> {
    let labels: Vec<_> = found.iter().map(label).collect();
    let width = labels
        .iter()
        .map(|label| label.chars().count())
        .max()
        .unwrap_or(0)
        .min(48);
    found
        .iter()
        .zip(&labels)
        .map(|(entry, label)| {
            let release = match library::edition_parts(&entry.edition) {
                Some((family, date)) => format!("{family} {}", library::show_date(&date)),
                None => clean(&entry.edition),
            };
            let source = entry
                .source
                .as_ref()
                .map(|source| clean(&source.release_name))
                .unwrap_or_default();
            let row = format!(
                "{label:<width$}  {release:<16}  {:>9} concepts  {source}",
                number(entry.active_concepts)
            );
            let row = row.trim_end().to_owned();
            if entry.selected {
                format!("{row}  (selected)")
            } else {
                row
            }
        })
        .collect()
}

fn label(found: &Found) -> String {
    match &found.name {
        Some(name) => clean(name),
        None => clean(&found.path.display().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snomed_ecl_engine::store::Source;

    fn found(name: &str, edition: &str, sha: &str, selected: bool) -> Found {
        Found {
            name: Some(name.into()),
            path: format!("{name}.ecl").into(),
            edition: edition.into(),
            archive_sha256: sha.into(),
            source: Some(Source {
                distributor: "trud".into(),
                item: 1799,
                release_id: "synthetic.zip".into(),
                release_name: "Release 3.0.0".into(),
                release_date: "2026-10-06".into(),
                archive_file_name: "synthetic.zip".into(),
            }),
            active_concepts: 1234,
            concepts: 2000,
            bytes: 10,
            packed: true,
            selected,
        }
    }

    #[test]
    fn index_rows_show_release_size_and_selection() {
        let rows = index_rows(&[
            found(
                "uk-20260923",
                "http://snomed.info/sct/83821000000107/version/20260923",
                "a",
                true,
            ),
            found(
                "int-20260801",
                "http://snomed.info/sct/900000000000207008/version/20260801",
                "b",
                false,
            ),
        ]);
        assert!(
            rows[0].starts_with("uk-20260923   uk 2026-09-23"),
            "{}",
            rows[0]
        );
        assert!(rows[0].contains("Release 3.0.0"));
        assert!(rows[0].ends_with("(selected)"));
        assert!(
            rows[1].starts_with("int-20260801  int 2026-08-01"),
            "{}",
            rows[1]
        );
        assert!(!rows[1].contains("(selected)"));
    }

    #[cfg(feature = "download")]
    #[test]
    fn release_rows_mark_the_newest_and_indexed_archives() {
        let body = include_str!("updates/test-releases.json");
        let releases = crate::download::parse_releases(body, true, "SENTINEL-KEY-123")
            .unwrap()
            .0;
        let library = [found(
            "uk-20260801",
            "http://snomed.info/sct/83821000000107/version/20260801",
            &releases[1].archive_file_sha256.to_ascii_lowercase(),
            false,
        )];
        let rows = release_rows(&releases, &library);
        assert!(
            rows[0].starts_with("2026-10-06  Release 3.0.0"),
            "{}",
            rows[0]
        );
        assert!(rows[0].ends_with("newest"), "{}", rows[0]);
        assert!(rows[1].ends_with("indexed as uk-20260801"), "{}", rows[1]);
        assert!(!rows[2].contains("indexed") && !rows[2].contains("newest"));
        assert!(indexed(&releases[1], &library).is_some());
        assert!(indexed(&releases[0], &library).is_none());
        for row in rows {
            assert!(!row.contains("SENTINEL") && !row.contains("/keys/"));
        }
        let items = item_rows();
        assert!(items[0].starts_with("uk-monolith    SNOMED CT UK Monolith"));
        assert!(items[1].starts_with("international  SNOMED CT International"));
    }
}
