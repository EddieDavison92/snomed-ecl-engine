# Prepare the local workspace

Use PowerShell 7, Python 3.11 or newer, Git, GitHub CLI and 1Password CLI. The optional local server uses Docker Desktop with Linux containers. Run commands from the repository root.

## Download the release

Unlock 1Password and enable its CLI integration. The script reads `op://Work/digital.nhs.uk/api-key` without printing it.

```powershell
./scripts/Get-Rf2Release.ps1
python scripts/inspect_rf2.py data/rf2/uk_sct2mo_42.5.0_20260826000001Z.zip
```

The first command selects the latest item 1799 release, verifies size and SHA-256, and writes a safe local manifest. To reproduce the pinned release after a newer one appears, run:

```powershell
./scripts/Get-Rf2Release.ps1 -ReleaseId uk_sct2mo_42.5.0_20260826000001Z.zip
```

Keep archives, extracted files, indexes and release-derived code lists inside ignored `data/`. Do not save raw TRUD responses or credential-bearing URLs.

## Restore reference checkouts

```powershell
foreach ($reference in (Get-Content docs/references.json -Raw | ConvertFrom-Json)) {
    git clone $reference.remote ('references/' + $reference.name)
    git -C ('references/' + $reference.name) checkout --detach $reference.commit
}
```

## Refresh the OneLondon probes

The local Windows Credential Manager setup used by the terminology-server skill must already work. This helper is machine-specific and is not included in the repository.

```powershell
./scripts/Test-Ontoserver.ps1
```

Results go to `data/validation`. The script checks the edition and retrieves all pages for each small probe. It does not perform a load test. Review a refreshed summary before replacing the tracked baseline.

## Inspect the local baseline container

```powershell
docker logs --tail 30 snomed-ecl-baseline
docker stats --no-stream snomed-ecl-baseline
docker stop snomed-ecl-baseline
```

The preparation container's command includes `--load`. Do not restart it blindly: create a serving-only container against the completed index to avoid another import. Preserve `data/snowstorm-lite` and read `baseline-status.md` before rerunning the experiment.

The preparation run has a background `Watch-Baseline.ps1` process. It waits for import completion, runs the eight-query local smoke comparison, and stops the container to release memory. It also stops after a 45-minute timeout. Read `data/baseline-status.json` and `data/validation/snowstorm-lite-smoke.json` for the eventual outcome. A timeout leaves an incomplete index; do not use it as a benchmark dataset.
