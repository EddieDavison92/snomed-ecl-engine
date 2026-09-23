//! Everything a browser shows for one concept, resolved in a single pass.
//!
//! An ECL expression answers "which concepts", never "what is this concept".
//! A term browser needs the second: the descriptions, the neighbours either
//! side of it in the hierarchy, its relationship groups and the reference sets
//! it belongs to. Nothing here evaluates ECL.
use crate::store::{ConcreteValue, DisplayStore, NumericStore};
use anyhow::Result;
use serde::Serialize;

/// The fully defined bit of a concept's flags. Bit 0 is active.
const DEFINED: u8 = 2;
const FSN: u64 = 900000000000003001;
const PREFERRED: u64 = 900000000000548007;

#[derive(Serialize)]
pub struct Concept {
    pub code: String,
    pub display: Option<String>,
}

#[derive(Serialize)]
pub struct Description {
    pub id: String,
    pub term: String,
    pub kind: Concept,
    pub active: bool,
    pub language: String,
    /// Language reference sets that call this term preferred.
    pub preferred_in: Vec<Concept>,
    /// Language reference sets that merely accept it.
    pub acceptable_in: Vec<Concept>,
}

/// A concrete attribute's value, kept in the type it was published as.
/// Numbers stay decimal strings: comparing them as binary floats loses rows.
#[derive(Serialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum Value {
    Number(String),
    Text(String),
    Boolean(bool),
}

impl From<&ConcreteValue> for Value {
    fn from(value: &ConcreteValue) -> Self {
        match value {
            ConcreteValue::Number(n) => Value::Number(n.clone()),
            ConcreteValue::Text(t) => Value::Text(t.clone()),
            ConcreteValue::Boolean(b) => Value::Boolean(*b),
        }
    }
}

#[derive(Serialize)]
pub struct Relationship {
    #[serde(rename = "type")]
    pub kind: Concept,
    /// A concept target, for an ordinary attribute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<Concept>,
    /// A typed literal, for a concrete attribute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Serialize)]
pub struct Group {
    /// Group 0 means ungrouped. Other numbers group attributes that apply together.
    pub group: u32,
    pub attributes: Vec<Relationship>,
}

#[derive(Serialize)]
pub struct Detail {
    pub code: String,
    pub active: bool,
    /// Fully defined, as opposed to primitive.
    pub defined: bool,
    pub module: Concept,
    pub effective_time: u32,
    /// The fully specified name, if the index carries descriptions.
    pub fsn: Option<String>,
    /// The label the engine itself uses for this concept.
    pub display: Option<String>,
    pub parents: Vec<Concept>,
    pub children: Vec<Concept>,
    pub groups: Vec<Group>,
    pub descriptions: Vec<Description>,
    pub refsets: Vec<Concept>,
}

/// Looks up one concept. `None` means the SCTID is not in this edition.
pub fn describe(
    store: &NumericStore,
    displays: &DisplayStore,
    sctid: u64,
) -> Result<Option<Detail>> {
    let Some(ordinal) = store.ordinal(sctid) else {
        return Ok(None);
    };
    let label = |ordinal: u32| -> Result<Concept> {
        Ok(Concept {
            code: store.ids[ordinal as usize].to_string(),
            display: displays.get(ordinal)?,
        })
    };

    let mut parents = Vec::new();
    for &parent in store.parents.get(ordinal) {
        parents.push(label(parent)?);
    }
    let mut children = Vec::new();
    for &child in store.children.get(ordinal) {
        children.push(label(child)?);
    }

    // Attributes arrive sorted by group, so runs of equal group are contiguous.
    let mut groups: Vec<Group> = Vec::new();
    let push = |groups: &mut Vec<Group>, group: u32, attribute: Relationship| {
        match groups.last_mut() {
            Some(last) if last.group == group => last.attributes.push(attribute),
            _ => groups.push(Group {
                group,
                attributes: vec![attribute],
            }),
        }
    };
    for attribute in store.attributes.get(ordinal) {
        let relationship = Relationship {
            kind: label(attribute.kind)?,
            target: Some(label(attribute.value)?),
            value: None,
        };
        push(&mut groups, attribute.group, relationship);
    }
    for attribute in store.concrete.get(ordinal) {
        let value = store
            .concrete_values
            .get(attribute.value as usize)
            .map(Value::from);
        let relationship = Relationship {
            kind: label(attribute.kind)?,
            target: None,
            value,
        };
        push(&mut groups, attribute.group, relationship);
    }
    groups.sort_by_key(|g| g.group);

    let mut fsn = None;
    let mut descriptions = Vec::new();
    if let Some(rows) = store.descriptions.concept_rows(ordinal)? {
        for row in rows {
            let kind_ordinal = row.kind;
            let kind = store.ids[kind_ordinal as usize];
            let term = row.term;
            let active = row.active;
            if kind == FSN && active && fsn.is_none() {
                fsn = Some(term.clone());
            }
            let (mut preferred_in, mut acceptable_in) = (Vec::new(), Vec::new());
            for (refset, acceptability) in row.dialects {
                let target = if store.ids[acceptability as usize] == PREFERRED {
                    &mut preferred_in
                } else {
                    &mut acceptable_in
                };
                target.push(label(refset)?);
            }
            let language = row.language;
            descriptions.push(Description {
                id: row.id.to_string(),
                term,
                kind: label(kind_ordinal)?,
                active,
                language: String::from_utf8_lossy(&language).into_owned(),
                preferred_in,
                acceptable_in,
            });
        }
        // Active first, then fully specified names, then alphabetically.
        descriptions.sort_by(|a, b| {
            b.active
                .cmp(&a.active)
                .then_with(|| (a.kind.code != FSN.to_string()).cmp(&(b.kind.code != FSN.to_string())))
                .then_with(|| a.term.cmp(&b.term))
        });
    }

    // Membership is stored the other way round, so this asks each reference set
    // whether it holds this concept. The member lists are sorted.
    let mut refsets = Vec::new();
    if let Some(membership) = &store.membership {
        for (position, &refset) in membership.refsets.iter().enumerate() {
            if membership.get(position).binary_search(&ordinal).is_ok() {
                refsets.push(label(refset)?);
            }
        }
    }

    let flags = store.flags[ordinal as usize];
    Ok(Some(Detail {
        code: sctid.to_string(),
        active: flags & 1 != 0,
        defined: flags & DEFINED != 0,
        module: label(store.modules[ordinal as usize])?,
        effective_time: store.effective_times[ordinal as usize],
        fsn,
        display: displays.get(ordinal)?,
        parents,
        children,
        groups,
        descriptions,
        refsets,
    }))
}
