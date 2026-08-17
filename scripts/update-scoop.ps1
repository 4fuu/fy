param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$')]
    [string]$Version,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path $_ -PathType Leaf })]
    [string]$Executable
)

$ErrorActionPreference = 'Stop'
$manifestPath = Join-Path $PSScriptRoot '..\bucket\fy.json'
$manifest = Get-Content $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$hash = (Get-FileHash $Executable -Algorithm SHA256).Hash.ToLowerInvariant()

$manifest.version = $Version
$manifest.architecture.'64bit'.url = "https://github.com/4fuu/fy/releases/download/v$Version/fy.exe"
$manifest.architecture.'64bit'.hash = $hash
$manifest | ConvertTo-Json -Depth 10 | Set-Content $manifestPath -Encoding UTF8

Write-Host "Updated bucket/fy.json to v$Version ($hash)"
