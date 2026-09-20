use super::{put_u32, put_u32s, put_u64, sha256, IndexSource, Input};
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::OnceLock;

const MAGIC: &[u8; 8] = b"SNMEM001";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemberManifest {
    pub refset: u64,
    pub bytes: u64,
    pub sha256: String,
    pub rows: usize,
    pub fields: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct TextColumn {
    pub offsets: Vec<u32>,
    pub text: String,
}
impl Default for TextColumn {
    fn default() -> Self {
        Self {
            offsets: vec![0],
            text: String::new(),
        }
    }
}
impl TextColumn {
    pub fn push(&mut self, value: &str) -> Result<()> {
        if self.offsets.is_empty() {
            self.offsets.push(0);
        }
        self.text.push_str(value);
        self.offsets.push(u32::try_from(self.text.len())?);
        Ok(())
    }
    pub fn get(&self, row: usize) -> &str {
        &self.text[self.offsets[row] as usize..self.offsets[row + 1] as usize]
    }
}

#[derive(Clone, Debug)]
pub enum MemberColumn {
    Id(Vec<u64>),
    Integer(Vec<i64>),
    Number(TextColumn),
    Boolean(Vec<u8>),
    Time(Vec<u32>),
    Text(TextColumn),
    Uuid(Vec<[u8; 16]>),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum MemberValue {
    Concept(String),
    Number(String),
    Boolean(bool),
    Time(String),
    String(String),
}

impl MemberColumn {
    pub fn len(&self) -> usize {
        match self {
            Self::Id(v) => v.len(),
            Self::Integer(v) => v.len(),
            Self::Boolean(v) => v.len(),
            Self::Time(v) => v.len(),
            Self::Text(v) | Self::Number(v) => v.offsets.len().saturating_sub(1),
            Self::Uuid(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn value(&self, row: usize) -> MemberValue {
        match self {
            Self::Id(v) => MemberValue::Concept(v[row].to_string()),
            Self::Integer(v) => MemberValue::Number(v[row].to_string()),
            Self::Number(v) => MemberValue::Number(v.get(row).into()),
            Self::Boolean(v) => MemberValue::Boolean(v[row] != 0),
            Self::Time(v) => MemberValue::Time(if v[row] == 0 {
                String::new()
            } else {
                format!("{:08}", v[row])
            }),
            Self::Text(v) => MemberValue::String(v.get(row).into()),
            Self::Uuid(v) => MemberValue::String(format_uuid(&v[row])),
        }
    }
}

pub fn parse_uuid(text: &str) -> Result<[u8; 16]> {
    ensure!(
        text.len() == 36 && [8, 13, 18, 23].iter().all(|&i| text.as_bytes()[i] == b'-'),
        "Invalid member UUID"
    );
    let hex: String = text.chars().filter(|&c| c != '-').collect();
    ensure!(
        hex.len() == 32 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid member UUID"
    );
    let mut uuid = [0; 16];
    for (i, byte) in uuid.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)?;
    }
    Ok(uuid)
}
pub fn format_uuid(uuid: &[u8; 16]) -> String {
    let mut text = String::with_capacity(36);
    for (i, b) in uuid.iter().enumerate() {
        if [4, 6, 8, 10].contains(&i) {
            text.push('-');
        }
        use std::fmt::Write;
        write!(text, "{b:02x}").unwrap();
    }
    text
}

/// One concept-based refset. Field columns preserve member rows and their metadata.
#[derive(Clone, Debug)]
pub struct MemberTable {
    pub refset: u64,
    pub names: Vec<String>,
    pub columns: Vec<MemberColumn>,
}
impl MemberTable {
    pub fn append(&mut self, other: Self) -> Result<()> {
        self.validate()?;
        other.validate()?;
        ensure!(
            self.refset == other.refset && self.names == other.names,
            "Refset schemas differ"
        );
        for (left, right) in self.columns.iter_mut().zip(other.columns) {
            match (left, right) {
                (MemberColumn::Id(a), MemberColumn::Id(b)) => a.extend(b),
                (MemberColumn::Integer(a), MemberColumn::Integer(b)) => a.extend(b),
                (MemberColumn::Boolean(a), MemberColumn::Boolean(b)) => a.extend(b),
                (MemberColumn::Time(a), MemberColumn::Time(b)) => a.extend(b),
                (MemberColumn::Uuid(a), MemberColumn::Uuid(b)) => a.extend(b),
                (MemberColumn::Text(a), MemberColumn::Text(b))
                | (MemberColumn::Number(a), MemberColumn::Number(b)) => {
                    let start = u32::try_from(a.text.len())?;
                    for offset in b.offsets.into_iter().skip(1) {
                        a.offsets.push(
                            start.checked_add(offset).ok_or_else(|| {
                                anyhow::anyhow!("Member text exceeds u32 capacity")
                            })?,
                        );
                    }
                    a.text.push_str(&b.text);
                }
                _ => bail!("Refset field types differ"),
            }
        }
        self.validate()
    }
    pub fn len(&self) -> usize {
        self.columns.first().map_or(0, MemberColumn::len)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn column(&self, name: &str) -> Option<&MemberColumn> {
        self.names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
            .map(|i| &self.columns[i])
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.names.len() == self.columns.len()
                && self.names.len() >= 6
                && self.names.len() <= 64,
            "Invalid member schema"
        );
        ensure!(
            self.names[..6]
                == [
                    "id",
                    "effectiveTime",
                    "active",
                    "moduleId",
                    "refsetId",
                    "referencedComponentId"
                ],
            "Invalid member metadata fields"
        );
        let mut names = HashSet::new();
        for (name, col) in self.names.iter().zip(&self.columns) {
            ensure!(
                !name.is_empty()
                    && name.bytes().all(|b| b.is_ascii_alphabetic())
                    && names.insert(name.to_ascii_lowercase()),
                "Invalid or duplicate member field"
            );
            ensure!(col.len() == self.len(), "Member column lengths differ");
            match col {
                MemberColumn::Id(v) => ensure!(
                    v.iter()
                        .all(|id| *id > 0 && *id < 1_000_000_000_000_000_000),
                    "Invalid member SCTID"
                ),
                MemberColumn::Boolean(v) => {
                    ensure!(v.iter().all(|&v| v <= 1), "Invalid member Boolean")
                }
                MemberColumn::Time(v) => {
                    ensure!(v.iter().all(|&v| valid_time(v)), "Invalid member date")
                }
                MemberColumn::Text(v) | MemberColumn::Number(v) => {
                    ensure!(
                        v.offsets.first() == Some(&0)
                            && v.offsets.last().copied() == u32::try_from(v.text.len()).ok(),
                        "Invalid member text offsets"
                    );
                    ensure!(
                        v.offsets.windows(2).all(|w| w[0] <= w[1])
                            && v.offsets
                                .iter()
                                .all(|&i| v.text.is_char_boundary(i as usize)),
                        "Invalid member UTF-8 boundary"
                    );
                    if matches!(col, MemberColumn::Number(_)) {
                        ensure!(
                            (0..v.offsets.len() - 1)
                                .all(|i| crate::decimal::Decimal::parse(v.get(i)).is_some()),
                            "Invalid numeric member field"
                        );
                    }
                }
                _ => {}
            }
        }
        ensure!(
            matches!(&self.columns[0], MemberColumn::Uuid(v) if v.iter().collect::<HashSet<_>>().len() == v.len()),
            "Duplicate or invalid member UUID column"
        );
        ensure!(
            matches!(&self.columns[1], MemberColumn::Time(_))
                && matches!(&self.columns[2], MemberColumn::Boolean(_)),
            "Invalid member status metadata"
        );
        ensure!(
            matches!(&self.columns[3], MemberColumn::Id(_))
                && matches!(&self.columns[4], MemberColumn::Id(v) if v.iter().all(|&r| r == self.refset)),
            "Invalid member module or refset metadata"
        );
        ensure!(
            matches!(&self.columns[5], MemberColumn::Id(v) if v.iter().all(|id| matches!((id / 10) % 100, 0 | 10))),
            "Refset does not reference concepts"
        );
        Ok(())
    }
    pub fn write(&self, directory: &Path) -> Result<MemberManifest> {
        self.validate()?;
        std::fs::create_dir_all(directory.join("members"))?;
        let path = directory
            .join("members")
            .join(format!("{}.bin", self.refset));
        let mut out = BufWriter::new(File::create_new(&path)?);
        out.write_all(MAGIC)?;
        put_u64(&mut out, self.refset)?;
        let schema = serde_json::to_vec(&self.names)?;
        put_u64(&mut out, schema.len() as u64)?;
        out.write_all(&schema)?;
        for column in &self.columns {
            match column {
                MemberColumn::Id(v) => {
                    put_u32(&mut out, 0)?;
                    put_u64(&mut out, v.len() as u64)?;
                    for &n in v {
                        put_u64(&mut out, n)?;
                    }
                }
                MemberColumn::Integer(v) => {
                    put_u32(&mut out, 1)?;
                    put_u64(&mut out, v.len() as u64)?;
                    for &n in v {
                        put_u64(&mut out, n as u64)?;
                    }
                }
                MemberColumn::Boolean(v) => {
                    put_u32(&mut out, 2)?;
                    put_u64(&mut out, v.len() as u64)?;
                    out.write_all(v)?;
                }
                MemberColumn::Time(v) => {
                    put_u32(&mut out, 3)?;
                    put_u32s(&mut out, v)?;
                }
                MemberColumn::Text(v) | MemberColumn::Number(v) => {
                    put_u32(
                        &mut out,
                        if matches!(column, MemberColumn::Number(_)) {
                            6
                        } else {
                            4
                        },
                    )?;
                    put_u32s(&mut out, &v.offsets)?;
                    put_u64(&mut out, v.text.len() as u64)?;
                    out.write_all(v.text.as_bytes())?;
                }
                MemberColumn::Uuid(v) => {
                    put_u32(&mut out, 5)?;
                    put_u64(&mut out, v.len() as u64)?;
                    for uuid in v {
                        out.write_all(uuid)?;
                    }
                }
            }
        }
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(MemberManifest {
            refset: self.refset,
            bytes: path.metadata()?.len(),
            sha256: sha256(&path)?,
            rows: self.len(),
            fields: self.names.clone(),
        })
    }
    pub(super) fn open(source: &IndexSource, metadata: &MemberManifest) -> Result<Self> {
        let mut input = Input::open(
            &source.section(&format!("members/{}.bin", metadata.refset))?,
            MAGIC,
        )?;
        let refset = input.u64()?;
        let names: Vec<String> = serde_json::from_slice(&input.bytes()?)?;
        ensure!(
            names.len() <= 64 && names == metadata.fields,
            "Member schema differs from manifest"
        );
        let mut columns = Vec::new();
        for _ in &names {
            columns.push(match input.u32()? {
                0 => MemberColumn::Id({
                    let n = input.count(8)?;
                    (0..n).map(|_| input.u64()).collect::<Result<_>>()?
                }),
                1 => MemberColumn::Integer({
                    let n = input.count(8)?;
                    (0..n)
                        .map(|_| input.u64().map(|v| v as i64))
                        .collect::<Result<_>>()?
                }),
                2 => MemberColumn::Boolean(input.bytes()?),
                3 => MemberColumn::Time(input.u32s()?),
                4 => MemberColumn::Text(TextColumn {
                    offsets: input.u32s()?,
                    text: String::from_utf8(input.bytes()?)?,
                }),
                5 => {
                    let n = input.count(16)?;
                    let mut v = Vec::with_capacity(n);
                    for _ in 0..n {
                        let mut uuid = [0; 16];
                        input.read(&mut uuid)?;
                        v.push(uuid);
                    }
                    MemberColumn::Uuid(v)
                }
                6 => MemberColumn::Number(TextColumn {
                    offsets: input.u32s()?,
                    text: String::from_utf8(input.bytes()?)?,
                }),
                _ => bail!("Unknown member field type"),
            });
        }
        let table = Self {
            refset,
            names,
            columns,
        };
        ensure!(
            input.remaining == 0 && refset == metadata.refset && table.len() == metadata.rows,
            "Member manifest counts differ or trailing bytes"
        );
        table.validate()?;
        Ok(table)
    }
}

pub(crate) fn valid_time(value: u32) -> bool {
    if value == 0 {
        return true;
    }
    let (year, month, day) = (value / 10000, value / 100 % 100, value % 100);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 28 + u32::from(year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)),
        _ => 0,
    };
    (1000..=9999).contains(&year) && day >= 1 && day <= days
}

#[derive(Debug, Default)]
pub struct MemberStore {
    source: Option<IndexSource>,
    tables: BTreeMap<
        u64,
        (
            MemberManifest,
            OnceLock<std::result::Result<MemberTable, String>>,
        ),
    >,
    available: bool,
}
impl MemberStore {
    pub fn loaded(tables: Vec<MemberTable>) -> Result<Self> {
        let mut result = Self {
            available: true,
            ..Self::default()
        };
        for table in tables {
            table.validate()?;
            let meta = MemberManifest {
                refset: table.refset,
                bytes: 0,
                sha256: String::new(),
                rows: table.len(),
                fields: table.names.clone(),
            };
            ensure!(
                result
                    .tables
                    .insert(table.refset, (meta, OnceLock::from(Ok(table))))
                    .is_none(),
                "Duplicate member table"
            );
        }
        Ok(result)
    }
    pub(super) fn lazy(source: &IndexSource, metadata: Vec<MemberManifest>) -> Result<Self> {
        let mut store = Self {
            source: Some(source.clone()),
            available: true,
            ..Self::default()
        };
        for meta in metadata {
            ensure!(
                meta.refset > 0
                    && store
                        .tables
                        .insert(meta.refset, (meta, OnceLock::new()))
                        .is_none(),
                "Duplicate member manifest"
            );
        }
        Ok(store)
    }
    pub fn is_available(&self) -> bool {
        self.available
    }
    pub fn refsets(&self) -> impl Iterator<Item = u64> + '_ {
        self.tables.keys().copied()
    }
    pub fn fields(&self, refset: u64) -> Option<&[String]> {
        self.tables.get(&refset).map(|(m, _)| m.fields.as_slice())
    }
    pub fn get(&self, refset: u64) -> Result<Option<&MemberTable>> {
        let Some((meta, slot)) = self.tables.get(&refset) else {
            return Ok(None);
        };
        match slot.get_or_init(|| {
            MemberTable::open(self.source.as_ref().unwrap(), meta).map_err(|e| e.to_string())
        }) {
            Ok(table) => Ok(Some(table)),
            Err(e) => bail!("Member index: {e}"),
        }
    }
}
