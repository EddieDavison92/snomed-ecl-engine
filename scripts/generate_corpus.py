"""Generate a deterministic ECL workload from pinned RF2, preserving regression cases."""
import argparse
from collections import Counter
import csv
import hashlib
import io
import json
from pathlib import Path
import random
import re
import zipfile

ROOT = Path(__file__).resolve().parent.parent
SEED = 20260826


def generate(size=10000):
    if size not in (1000, 10000):
        raise ValueError("Supported corpus sizes are 1000 and 10000")
    release = json.loads((ROOT / "docs/release.json").read_text(encoding="utf-8-sig"))
    path = ROOT / "data/rf2" / release["archiveFileName"]
    with path.open("rb") as source:
        assert hashlib.file_digest(source, "sha256").hexdigest() == release["sha256"].lower()
    rng = random.Random(SEED)
    with zipfile.ZipFile(path) as archive:
        def sample(prefix, isa=None):
            names = [n for n in archive.namelist() if "/Snapshot/" in n and Path(n).name.startswith(prefix)]
            assert len(names) == 1
            result, count = [], 0
            with archive.open(names[0]) as member:
                rows = csv.DictReader(io.TextIOWrapper(member, encoding="utf-8-sig"), delimiter="\t", quoting=csv.QUOTE_NONE)
                for row in rows:
                    if row["active"] != "1" or row["characteristicTypeId"] != "900000000000011006":
                        continue
                    if isa is not None and (row["typeId"] == "116680003") != isa:
                        continue
                    count += 1
                    if len(result) < (1000 if size == 1000 else 12000):
                        result.append(row)
                    else:
                        position = rng.randrange(count)
                        if position < len(result):
                            result[position] = row
            # Distinct source concepts prevent repeated expressions within a category.
            unique = {r["sourceId"]: r for r in result}
            ordered = sorted(unique.values(), key=lambda r: int(r["sourceId"]))
            if size == 10000:
                rng.shuffle(ordered)
            return ordered[:40 if size == 1000 else 600]

        hierarchy = sample("sct2_Relationship_", True)
        attributes = sample("sct2_Relationship_", False)
        concrete = sample("sct2_RelationshipConcreteValues_")
        term_prefixes, associations = {}, []
        if size == 10000:
            selected = {row['sourceId'] for row in hierarchy}
            for name in archive.namelist():
                if '/Snapshot/' not in name:
                    continue
                description = Path(name).name.startswith('sct2_Description_')
                association = Path(name).name.startswith('der2_cRefset_Association')
                if not (description or association):
                    continue
                with archive.open(name) as member:
                    rows = csv.DictReader(io.TextIOWrapper(member, encoding='utf-8-sig'),
                                          delimiter='\t', quoting=csv.QUOTE_NONE)
                    seen = 0
                    for row in rows:
                        if row['active'] != '1':
                            continue
                        if description:
                            concept = row['conceptId']
                            if concept not in selected or concept in term_prefixes or row['languageCode'] != 'en':
                                continue
                            words = re.findall(r'[A-Za-z]{4,}', row['term'])
                            if words:
                                term_prefixes[concept] = words[0][:4].lower()
                        elif row['refsetId'] == '900000000000526001':
                            seen += 1
                            if len(associations) < 1200:
                                associations.append(row['referencedComponentId'])
                            else:
                                position = rng.randrange(seen)
                                if position < len(associations):
                                    associations[position] = row['referencedComponentId']
    baseline_path = ROOT / "validation/ecl-1000.json"
    baseline = json.loads(baseline_path.read_text(encoding="utf-8")) if size == 10000 else None
    if baseline and baseline["archive_sha256"] != release["sha256"].lower():
        raise ValueError("Regression corpus belongs to a different release")
    cases = list(baseline["cases"]) if baseline else []
    baseline_categories = {row['category'] for row in cases}
    counts = Counter(r["category"] for r in cases)
    expressions = {r["ecl"] for r in cases}

    def add(category, ecl, limit=None):
        limit = limit or (40 if size == 1000 else 320)
        if ecl in expressions or counts[category] >= limit:
            return
        number = counts[category] + 1
        cases.append({"id": f"{category}-{number:02}", "category": category, "ecl": ecl})
        counts[category] += 1
        expressions.add(ecl)

    if size == 10000:
        # Active historical members provide positive projections; the legacy cases remain.
        for source in sorted(set(associations), key=int):
            add('member-projection', f'^ [targetComponentId] 900000000000526001 {{{{ M referencedComponentId = {source} }}}}')

    for i, row in enumerate(hierarchy):
        a, b = row["sourceId"], row["destinationId"]
        for category, ecl in {
            "descendants": f"<< {b}",
            "ancestors": f"> {a}",
            "children": f"<! {a}",
            "parents": f">! {a}",
            "union": f"(<< {a}) OR (<< {b})",
            "intersection": f"(<< {a}) AND (<< {b})",
            "exclusion": f"(<< {b}) MINUS (<< {a})",
            "nested-hierarchy": f">! (<! {a})",
            "top": f"!!> ({a} OR {b})",
            "bottom": f"!!< ({a} OR {b})",
            "membership": f"(<< {b}) AND (^ 999002271000000101)",
            "concept-filter": f"{a} {{{{ C definitionStatus = defined }}}}",
            "description-filter": f"{a} {{{{ D type = syn, language = en }}}}",
            "history": f"{a} {{{{ +HISTORY-MIN }}}}",
            "member-projection": f"^ [targetComponentId] 900000000000526001 {{{{ M referencedComponentId = {a} }}}}",
        }.items():
            # Parent concepts can repeat in a reservoir sample.
            if category in ("descendants", "membership"):
                ecl = f"<< {a}" if category == "descendants" else f"(<< {a}) AND (^ 999002271000000101)"
            add(category, ecl)

    for i, row in enumerate(attributes):
        a, t, v = row["sourceId"], row["typeId"], row["destinationId"]
        for category, ecl in {
            "dotted": f"(<< {a}) . {t}",
            "attribute-equality": f"(<< {a}) : {t} = {v}",
            "attribute-inequality": f"(<< {a}) : {t} != {v}",
            "nested-value": f"(<< {a}) : {t} = (<< {v})",
            "wildcard-name": f"(<< {a}) : * = {v}",
            "attribute-cardinality": f"(<< {a}) : [{i % 3}..{i % 3 + 1}] {t} = *",
            "group": f"(<< {a}) : {{ {t} = {v} }}",
            "group-cardinality": f"(<< {a}) : [{i % 2}..{i % 2 + 1}] {{ {t} = * }}",
            "reverse": f"* : R {t} = {a}",
        }.items():
            add(category, ecl)
    for i, row in enumerate(concrete):
        operator = ["=", "!=", "<", "<=", ">", ">="][i % 6] if row["value"].startswith("#") else "="
        add("concrete", f"(<< {row['sourceId']}) : {row['typeId']} {operator} {row['value']}")
    if size == 10000:
        for i, row in enumerate(hierarchy):
            a = row["sourceId"]
            focus = f"(<< {a})"
            token = term_prefixes.get(a, 'zzzznomatch') if i % 4 else 'zzzznomatch'
            for category, ecl in {
                "strict-descendants": f"< {a}",
                "ancestors-or-self": f">> {a}",
                "children-or-self": f"<<! {a}",
                "parents-or-self": f">>! {a}",
                "concept-active": f"{focus} {{{{C active={i % 2}}}}}",
                "concept-module": f"{focus} {{{{C moduleId=<< 900000000000443000}}}}",
                "concept-effective-time": f'{focus} {{{{C effectiveTime>="20200131"}}}}',
                "description-type-negation": f"{focus} {{{{D type!=fsn}}}}",
                "description-dialect": f"{focus} {{{{D dialect=en-gb (prefer)}}}}",
                "description-all-active-states": f"{focus} {{{{D active=*, type=syn}}}}",
                "term-prefix": f'{focus} {{{{D term=match:"{token}"}}}}',
                "term-wildcard": f'{focus} {{{{D term=wild:"*{token}*"}}}}',
                "term-set": f'{focus} {{{{D term=(match:"{token}" wild:"*itis")}}}}',
                "history-moderate": f"{a} {{{{+HISTORY-MOD}}}}",
                "history-maximum": f"{a} {{{{+HISTORY-MAX}}}}",
                "refset-containing": f"^R {a}",
            }.items():
                add(category, ecl, 100)
        for row in attributes:
            a, t, v = row["sourceId"], row["typeId"], row["destinationId"]
            for category, ecl in {
                "refinement-conjunction": f"(<< {a}) : ({t} = {v} AND [1..*] {t} = *)",
                "refinement-disjunction": f"(<< {a}) : ({t} = {v} OR [0..0] {t} = *)",
                "group-conjunction": f"(<< {a}) : {{ {t} = {v}, [1..*] {t} = * }}",
                "reverse-cardinality": f"* : [1..1] R {t} = {a}",
            }.items():
                add(category, ecl, 100)
    assert len(cases) == len({r["ecl"] for r in cases}) == size, counts
    assert len({r["id"] for r in cases}) == size
    expected = {category: 40 for category in counts}
    if baseline:
        expected = {category: 320 if category in baseline_categories else 100 for category in counts}
        assert cases[:1000] == baseline["cases"]
        assert len(baseline_categories) == 25 and len(counts) == 45
    assert counts == expected
    result = {"seed": SEED, "archive_sha256": release["sha256"].lower(),
              "scope": ("40 cases in each of 25 categories. " if size == 1000 else
                        "The original 1,000 cases retained verbatim; 320 cases in each of the original 25 categories and 100 in each of 20 further categories. ") +
                       "Deterministic relationship-row reservoir with distinct source concepts. A coverage and performance workload, not a proof of full ECL conformance or a clinical workload distribution. Pending capabilities remain included.",
              "cases": cases}
    if baseline:
        result["baseline_corpus_sha256"] = hashlib.sha256(baseline_path.read_bytes()).hexdigest()
        result['scope'] += (' New member projections sample active REPLACED BY rows. '
                            'Term queries alternate description-derived prefixes with deliberate non-matches.')
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--size", type=int, choices=(1000, 10000), default=10000)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = generate(args.size)
    output = args.output or ROOT / f"validation/ecl-{args.size}.json"
    output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"cases": len(result["cases"]), "categories": len({r['category'] for r in result['cases']}), "seed": SEED}))
