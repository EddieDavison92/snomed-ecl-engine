"""Record RF2 file sizes, row counts, modules and dependencies without extracting it."""

import collections
import csv
import io
import json
import pathlib
import sys
import zipfile

archive = pathlib.Path(sys.argv[1])
csv.field_size_limit(16 * 1024 * 1024)
report = {"archive": archive.name, "files": [], "concept_modules": {}, "module_dependencies": []}
with zipfile.ZipFile(archive) as release:
    for entry in release.infolist():
        if entry.is_dir():
            continue
        record = {"name": entry.filename, "bytes": entry.file_size, "compressed_bytes": entry.compress_size}
        if "/Snapshot/" in entry.filename and entry.filename.endswith(".txt"):
            with release.open(entry) as raw:
                reader = csv.DictReader(io.TextIOWrapper(raw, encoding="utf-8-sig"), delimiter="\t", quoting=csv.QUOTE_NONE)
                count = active = 0
                modules = collections.Counter()
                for row in reader:
                    count += 1
                    active += row.get("active") == "1"
                    if "sct2_Concept_" in entry.filename:
                        modules[(row["moduleId"], row["active"])] += 1
                    if "ModuleDependency" in entry.filename and row.get("active") == "1":
                        report["module_dependencies"].append({k: row[k] for k in (
                            "moduleId", "referencedComponentId", "sourceEffectiveTime", "targetEffectiveTime"
                        )})
                record.update(rows=count, active_rows=active, columns=reader.fieldnames)
                if modules:
                    report["concept_modules"][entry.filename] = [
                        {"moduleId": module, "active": state == "1", "count": number}
                        for (module, state), number in sorted(modules.items())
                    ]
        report["files"].append(record)
destination = archive.with_suffix(".inventory.json")
destination.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print(json.dumps({"inventory": str(destination), "files": len(report["files"]),
                  "uncompressed_bytes": sum(f["bytes"] for f in report["files"]),
                  "snapshot_rows": sum(f.get("rows", 0) for f in report["files"])}))
