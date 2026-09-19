param([int]$TimeoutMinutes = 45)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$deadline = (Get-Date).AddMinutes($TimeoutMinutes)
$statusPath = Join-Path $root 'data/baseline-status.json'
$status = [ordered]@{status='importing'; checkedAt=$null; benchmarkExitCode=$null; stopped=$false}
try {
    while ((Get-Date) -lt $deadline) {
        $state = docker inspect snomed-ecl-baseline --format '{{.State.Status}}'
        $log = docker logs --tail 40 snomed-ecl-baseline 2>&1 | Out-String
        $status.checkedAt = (Get-Date).ToUniversalTime().ToString('o')
        if ($state -ne 'running') { $status.status='container-exited'; break }
        if ($log -match 'Import complete') {
            $status.status='benchmarking'
            $status | ConvertTo-Json | Set-Content $statusPath -Encoding utf8
            & python (Join-Path $PSScriptRoot 'benchmark_local.py')
            $status.benchmarkExitCode=$LASTEXITCODE
            $status.status=if ($LASTEXITCODE -eq 0) { 'smoke-passed' } else { 'smoke-failed' }
            break
        }
        $status | ConvertTo-Json | Set-Content $statusPath -Encoding utf8
        Start-Sleep -Seconds 20
    }
    if ($status.status -eq 'importing') { $status.status='import-timeout' }
}
catch { $status.status='watcher-error' }
finally {
    # Retain the container and index, but release its CPU and RAM after the attempt.
    docker stop snomed-ecl-baseline | Out-Null
    $status.stopped=$LASTEXITCODE -eq 0
    $status.checkedAt=(Get-Date).ToUniversalTime().ToString('o')
    $status | ConvertTo-Json | Set-Content $statusPath -Encoding utf8
}
