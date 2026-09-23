# Installs snomed-ecl-engine from a GitHub release on Windows:
#
#   irm https://raw.githubusercontent.com/EddieDavison92/snomed-ecl-engine/main/install.ps1 | iex
#
# Environment:
#   SNOMED_ECL_VERSION      release to install, such as 0.1.1 (default: latest)
#   SNOMED_ECL_BUILD        default or query
#   SNOMED_ECL_INSTALL_DIR  where to put the executable
#                           (default: %LOCALAPPDATA%\snomed-ecl-engine\bin)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'EddieDavison92/snomed-ecl-engine'
$build = if ($env:SNOMED_ECL_BUILD) { $env:SNOMED_ECL_BUILD } else { 'default' }
$dir = if ($env:SNOMED_ECL_INSTALL_DIR) { $env:SNOMED_ECL_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'snomed-ecl-engine\bin' }
if ([Environment]::Is64BitOperatingSystem -eq $false -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
    throw 'snomed-ecl-engine has a Windows build for x64 only.'
}

$tag = if ($env:SNOMED_ECL_VERSION) { 'v' + $env:SNOMED_ECL_VERSION.TrimStart('v') } else {
    (Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest").tag_name
}
$name = "snomed-ecl-engine-$tag-x86_64-pc-windows-msvc-$build"
$url = "https://github.com/$repo/releases/download/$tag"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $sums = (Invoke-WebRequest "$url/SHA256SUMS" -UseBasicParsing).Content
    if ($sums -is [byte[]]) { $sums = [Text.Encoding]::UTF8.GetString($sums) }
    $line = $sums -split "`n" | Where-Object { ($_ -split '\s+')[1] -eq "$name.zip" }
    if (-not $line) { throw "$tag has no $build build for Windows x64." }
    $expected = ($line -split '\s+')[0]
    $archive = Join-Path $tmp "$name.zip"
    Invoke-WebRequest "$url/$name.zip" -OutFile $archive -UseBasicParsing
    $actual = (Get-FileHash $archive -Algorithm SHA256).Hash
    if ($actual -ne $expected) { throw "Checksum mismatch for $name.zip." }
    Expand-Archive $archive -DestinationPath $tmp
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Copy-Item (Join-Path $tmp "$name\snomed-ecl-engine.exe") (Join-Path $dir 'snomed-ecl-engine.exe') -Force
} finally {
    Remove-Item -Recurse -Force $tmp
}

$path = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($path -split ';') -notcontains $dir) {
    [Environment]::SetEnvironmentVariable('Path', ($path.TrimEnd(';') + ";$dir").TrimStart(';'), 'User')
    Write-Output "Added $dir to your user PATH; open a new terminal to use it."
}
Write-Output "Installed $(& (Join-Path $dir 'snomed-ecl-engine.exe') --version) to $dir"
