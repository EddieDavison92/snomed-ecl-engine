param(
    [string]$Version = 'http://snomed.info/sct/83821000000107/version/20260826',
    [string]$CredentialHelper = 'C:/Users/eddie/scripts/_terminology-creds.ps1',
    [string]$Destination = (Join-Path $PSScriptRoot '../data/validation'),
    [int]$PageSize = 500
)

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
. $CredentialHelper
$token = Get-TerminologyToken
$headers = @{ Authorization = "Bearer $token" }
try {
    $metadata = Invoke-RestMethod -Uri ($FhirServer + '/metadata') -Headers $headers
    $systems = Invoke-RestMethod -Uri ($FhirServer + '/CodeSystem?url=http%3A%2F%2Fsnomed.info%2Fsct&_count=100') -Headers $headers
    if ($systems.link | Where-Object relation -EQ 'next') { throw 'CodeSystem discovery was paginated; extend discovery before validation.' }
    if ($Version -notin @($systems.entry.resource.version)) { throw 'Requested edition is unavailable on the reference server.' }
    $queries = Get-Content (Join-Path $PSScriptRoot '../validation/queries.json') -Raw | ConvertFrom-Json
    $results = foreach ($query in $queries | Where-Object probe) {
        $codes = [Collections.Generic.HashSet[string]]::new()
        $offset = 0
        $reportedVersions = [Collections.Generic.HashSet[string]]::new()
        do {
            $valueSet = $Version + '?fhir_vs=ecl/' + $query.ecl
            $uri = $FhirServer + '/ValueSet/$expand?url=' + [uri]::EscapeDataString($valueSet) + '&count=' + $PageSize + '&offset=' + $offset
            $response = Invoke-RestMethod -Uri $uri -Headers $headers
            if ($null -eq $response.expansion.total) { throw 'Expansion did not return a total.' }
            $total = [int]$response.expansion.total
            $page = @($response.expansion.contains)
            foreach ($entry in $page) {
                if ($entry.contains) { throw 'Hierarchical expansion is not supported by this comparison script.' }
                if ($entry.version) { [void]$reportedVersions.Add($entry.version) }
                if ($entry.code) { [void]$codes.Add($entry.code) }
            }
            foreach ($parameter in $response.expansion.parameter) {
                if ($parameter.name -eq 'version' -and $parameter.valueUri) {
                    [void]$reportedVersions.Add(($parameter.valueUri -split '\|')[-1])
                }
            }
            $offset += $page.Count
            if ($offset -lt $total -and $page.Count -eq 0) { throw 'Expansion stopped before its reported total.' }
            if ($offset -gt 100000) { throw 'Smoke probe exceeded the expected result budget.' }
        } while ($offset -lt $total)
        if ($codes.Count -ne $total) { throw 'Expansion has duplicate codes or incomplete pagination.' }
        if ($reportedVersions.Count -eq 0 -or $Version -notin $reportedVersions) { throw 'Server did not confirm the requested edition.' }
        if (@($reportedVersions | Where-Object { $_ -ne $Version }).Count -gt 0) { throw 'Server returned a different edition.' }
        $sorted = @($codes | Sort-Object { [long]$_ })
        $bytes = [Text.Encoding]::UTF8.GetBytes(($sorted -join "`n") + "`n")
        $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
        $sorted | Set-Content (Join-Path $Destination ($query.id + '.codes.txt')) -Encoding utf8
        [ordered]@{id=$query.id;ecl=$query.ecl;total=$total;sha256=$hash;complete=$true;reportedVersions=@($reportedVersions)}
    }
    $report = [ordered]@{
        checkedAt=(Get-Date).ToUniversalTime().ToString('o')
        software=$metadata.software.name
        softwareVersion=$metadata.software.version
        edition=$Version
        digestFormat='Numeric SCTID sort; UTF-8; LF separated; final LF; no BOM.'
        results=@($results)
    }
    $report | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $Destination 'ontoserver.json') -Encoding utf8
    $report | ConvertTo-Json -Depth 8
}
catch { throw ('Ontoserver probe failed at script line ' + $_.InvocationInfo.ScriptLineNumber + '. Authenticated request details suppressed.') }
finally { $token=$null; $headers=$null }
