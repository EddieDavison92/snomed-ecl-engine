# Downloads an RF2 release from NHS England TRUD, checks its size and SHA-256
# against TRUD's metadata, and writes a manifest beside it. Item 1799 is the UK
# Monolith Edition Snapshot. Needs a TRUD API key with that item subscribed.
param(
    [int]$Item = 1799,
    [string]$ReleaseId,
    [string]$Destination = (Join-Path $PSScriptRoot '../data/rf2'),
    [string]$ApiKey = $env:TRUD_API_KEY
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
$key = $ApiKey
if (-not $key) { throw 'Set TRUD_API_KEY or pass -ApiKey.' }
try {
    $suffix = if ($ReleaseId) { '' } else { '?latest' }
    $uri = 'https://isd.digital.nhs.uk/trud/api/v1/keys/' + $key.Trim() + '/items/' + $Item + '/releases' + $suffix
    try { $response = Invoke-RestMethod -Uri $uri }
    catch { throw 'TRUD release lookup failed. Request details suppressed because the URL contains a credential.' }
    $release = if ($ReleaseId) {
        @($response.releases | Where-Object id -EQ $ReleaseId)
    } else { @($response.releases) }
    if (@($release).Count -ne 1) { throw 'Expected exactly one matching TRUD release.' }
    $release = @($release)[0]
    $name = $release.archiveFileName
    if ([IO.Path]::GetFileName($name) -ne $name -or $name -notmatch '\.zip$') { throw 'Unexpected archive filename.' }
    $downloadUri = [uri]$release.archiveFileUrl
    if ($downloadUri.Scheme -ne 'https' -or $downloadUri.Host -ne 'isd.digital.nhs.uk') { throw 'Unexpected TRUD download host.' }
    if ($release.archiveFileSha256 -notmatch '^[A-Fa-f0-9]{64}$') { throw 'Missing SHA-256 checksum.' }
    $archive = Join-Path $Destination $name
    if (-not (Test-Path -LiteralPath $archive)) {
        Write-Output "Downloading $name ($($release.archiveFileSizeBytes) bytes)."
        $partial = $archive + '.partial'
        try { Invoke-WebRequest -Uri $downloadUri -OutFile $partial }
        catch { throw 'TRUD download failed. Request details suppressed because the URL contains a credential.' }
        if ((Get-Item -LiteralPath $partial).Length -ne [long]$release.archiveFileSizeBytes) { throw 'Download size mismatch.' }
        if ((Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash -ne $release.archiveFileSha256) { throw 'Download checksum mismatch.' }
        Move-Item -LiteralPath $partial -Destination $archive
    }
    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
    if ($hash -ne $release.archiveFileSha256) { throw 'Archive checksum mismatch.' }
    # Do not serialise the API response: its download URLs contain the API key.
    $manifest = [ordered]@{
        item = $Item
        releaseId = $release.id
        releaseName = $release.name
        publishedDate = $release.releaseDate
        archiveFileName = $name
        bytes = (Get-Item -LiteralPath $archive).Length
        sha256 = $hash
        checkedAt = (Get-Date).ToUniversalTime().ToString('o')
    }
    $manifest | ConvertTo-Json | Set-Content -LiteralPath ($archive + '.manifest.json') -Encoding utf8
    $manifest | ConvertTo-Json
}
finally { $key = $null; $ApiKey = $null; $uri = $null; $response = $null; $release = $null; $downloadUri = $null }
