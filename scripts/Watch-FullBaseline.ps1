param(
    [Parameter(Mandatory)][string]$ImportId,
    [int]$TimeoutMinutes = 90
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$statusPath = Join-Path $root 'data/full-baseline-status.json'
$deadline = (Get-Date).AddMinutes($TimeoutMinutes)
$status = [ordered]@{ status='importing'; importId=$ImportId; checkedAt=$null; benchmarkExitCode=$null; stopped=$false }
try {
    while ((Get-Date) -lt $deadline) {
        $job = Invoke-RestMethod -Uri ('http://127.0.0.1:18082/imports/' + [uri]::EscapeDataString($ImportId)) -TimeoutSec 20
        $status.checkedAt = (Get-Date).ToUniversalTime().ToString('o')
        if ($job.status -eq 'FAILED') { $status.status='import-failed'; break }
        if ($job.status -eq 'COMPLETED') {
            $status.status='comparing'
            $status | ConvertTo-Json | Set-Content $statusPath -Encoding utf8
            & python (Join-Path $PSScriptRoot 'benchmark_corpus.py') --store-volume snomed-ecl-runtime-index --snowstorm http://127.0.0.1:18082 --import-id $ImportId --output (Join-Path $root 'data/validation/ecl-1000-full-snowstorm.json')
            $status.benchmarkExitCode=$LASTEXITCODE
            $status.status=if ($LASTEXITCODE -eq 0) { 'comparison-passed' } else { 'comparison-failed' }
            break
        }
        $status | ConvertTo-Json | Set-Content $statusPath -Encoding utf8
        Start-Sleep -Seconds 20
    }
    if ($status.status -eq 'importing') { $status.status='import-timeout' }
}
catch { $status.status='watcher-error'; $status.error=$_.Exception.Message }
finally {
    # Retain the imported index but release compute after the comparison attempt.
    docker stop --time 30 snomed-ecl-snowstorm snomed-ecl-elasticsearch | Out-Null
    $status.stopped=$LASTEXITCODE -eq 0
    $status.checkedAt=(Get-Date).ToUniversalTime().ToString('o')
    $status | ConvertTo-Json | Set-Content $statusPath -Encoding utf8
}
