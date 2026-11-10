$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$destination = Join-Path $projectRoot 'settings-backup'
New-Item -ItemType Directory -Path $destination -Force | Out-Null
$sourceDirectory = Join-Path $env:APPDATA 'DisplayProLite'
foreach ($fileName in @('profiles.json', 'photos.json', 'taskbars.json')) {
    $sourceFile = Join-Path $sourceDirectory $fileName
    if (Test-Path -LiteralPath $sourceFile) {
        Copy-Item -LiteralPath $sourceFile -Destination (Join-Path $destination $fileName) -Force
    }
}
Write-Output "Configuration backup: $destination"
