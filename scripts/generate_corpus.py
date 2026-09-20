"""Generate 1,000 distinct ECL cases from the pinned RF2, without storing release rows."""
import csv
import hashlib
import io
import json
from pathlib import Path
import random
import zipfile

ROOT = Path(__file__).resolve().parent.parent
SEED = 20260826


def generate():
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
                    if len(result) < 1000:
                        result.append(row)
                    else:
                        position = rng.randrange(count)
                        if position < len(result):
                            result[position] = row
            # Distinct source concepts prevent repeated expressions within a category.
            unique = {r["sourceId"]: r for r in result}
            return sorted(unique.values(), key=lambda r: int(r["sourceId"]))[:40]

        hierarchy = sample("sct2_Relationship_", True)
        attributes = sample("sct2_Relationship_", False)
        concrete = sample("sct2_RelationshipConcreteValues_")
    cases = []

    def add(category, ecl):
        number = sum(r["category"] == category for r in cases) + 1
        cases.append({"id": f"{category}-{number:02}", "category": category, "ecl": ecl})

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
    assert len(cases) == len({r["ecl"] for r in cases}) == 1000
    assert len({r["category"] for r in cases}) == 25
    return {"seed": SEED, "archive_sha256": release["sha256"].lower(),
            "scope": "40 cases in each of 25 categories. Deterministic relationship-row reservoir with distinct source concepts. A coverage and performance workload, not a proof of full ECL conformance or a clinical workload distribution. Pending capabilities remain included.",
            "cases": cases}


if __name__ == "__main__":
    result = generate()
    output = ROOT / "validation/ecl-1000.json"
    output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"cases": len(result["cases"]), "categories": 25, "seed": SEED}))
