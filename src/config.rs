//! Bind edition-specific names without changing the ECL parser.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct QueryConfig {
    #[serde(with = "id_map")]
    pub identifier_schemes: BTreeMap<String, u64>,
    #[serde(with = "id_map")]
    pub dialects: BTreeMap<String, u64>,
}
impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            identifier_schemes: BTreeMap::new(),
            dialects: BTreeMap::from([
                ("da-dk".into(), 554461000005103),
                ("en-au".into(), 32570271000036106),
                ("en-ca".into(), 19491000087109),
                ("en-gb".into(), 900000000000508004),
                ("en-ie".into(), 21000220103),
                ("en-nz".into(), 271000210107),
                ("en-nz-x-pat".into(), 281000210109),
                ("en-us".into(), 900000000000509007),
                ("en-x-gmdn".into(), 608771002),
                ("en-x-nhs-clinical".into(), 999001261000000100),
                ("en-x-nhs-dmd".into(), 999000671000001103),
                ("en-x-nhs-pharmacy".into(), 999000691000001104),
                ("en-gb-x-drug".into(), 999000681000001101),
                ("en-gb-x-ext".into(), 999001251000000103),
                ("es".into(), 450828004),
                ("es-uy".into(), 5641000179103),
                ("et-ee".into(), 71000181105),
                ("de".into(), 722130004),
                ("fr".into(), 722131000),
                ("fr-be".into(), 21000172104),
                ("fr-ca".into(), 20581000087109),
                ("ja".into(), 722129009),
                ("mi".into(), 291000210106),
                ("nl-be".into(), 31000172101),
                ("nl-nl".into(), 31000146106),
                ("nb-no".into(), 61000202103),
                ("nn-no".into(), 91000202106),
                ("sv-se".into(), 46011000052107),
                ("zh".into(), 722128001),
            ]),
        }
    }
}
impl QueryConfig {
    pub fn fingerprint(&self) -> Result<String> {
        use sha2::{Digest, Sha256};
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
    pub fn read(path: &Path) -> Result<Self> {
        let mut config: Self = serde_json::from_reader(std::fs::File::open(path)?)?;
        config.normalise()?;
        Ok(config)
    }
    pub fn normalise(&mut self) -> Result<()> {
        for map in [&mut self.identifier_schemes, &mut self.dialects] {
            let mut result = BTreeMap::new();
            for (name, id) in map.iter() {
                ensure!(valid_alias(name), "Invalid alias name");
                ensure!(
                    (100000..1_000_000_000_000_000_000).contains(id),
                    "Invalid alias SCTID"
                );
                ensure!(
                    result.insert(name.to_ascii_lowercase(), *id).is_none(),
                    "Duplicate alias ignoring case"
                );
            }
            *map = result;
        }
        Ok(())
    }
}
pub(crate) fn valid_alias(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
mod id_map {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        map: &BTreeMap<String, u64>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        map.iter()
            .map(|(k, v)| (k, v.to_string()))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<String, u64>, D::Error> {
        let values = BTreeMap::<String, String>::deserialize(deserializer)?;
        values
            .into_iter()
            .map(|(k, v)| {
                if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(serde::de::Error::custom("SCTIDs must be decimal strings"));
                }
                v.parse()
                    .map(|id| (k, id))
                    .map_err(serde::de::Error::custom)
            })
            .collect()
    }
}
