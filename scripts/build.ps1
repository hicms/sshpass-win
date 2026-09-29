<#
.SYNOPSIS
Checks and builds the release executable.

.DESCRIPTION
Runs rustfmt, clippy and the tests, builds the release executable and copies
it to dist\sshpass.exe together with dist\sshpass.exe.sha256.

.PARAMETER SkipChecks
Build without running rustfmt, clippy and the tests.

.EXAMPLE
.\scripts\build.ps1
#>
[CmdletBinding()]
param(
    [switch]$SkipChecks
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'common.ps1')

$root = Get-RepositoryRoot
Push-Location $root
try {
    if (-not $SkipChecks) {
        Invoke-Native cargo @('fmt', '--check')
        Invoke-Native cargo @('clippy', '--all-targets', '--', '-D', 'warnings')
        Invoke-Native cargo @('test')
    }
    Invoke-Native cargo @('build', '--release')

    $dist = Join-Path $root 'dist'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    $exe = Join-Path $dist 'sshpass.exe'
    Copy-Item -Force (Join-Path $root 'target\release\sshpass.exe') $exe

    $hash = (Get-FileHash -Algorithm SHA256 $exe).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText("$exe.sha256", "$hash  sshpass.exe`n")

    $size = [math]::Round((Get-Item $exe).Length / 1KB)
    Write-Host ''
    Write-Host "Built $exe ($size KiB, version $(Get-CargoVersion $root))" -ForegroundColor Green
    Write-Host "SHA256 $hash"
}
finally {
    Pop-Location
}
