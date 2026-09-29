# Shared helpers, dot-sourced by the other scripts.

# Runs a native command and stops the script if it fails.
# Arguments are passed as an array so that "--" and "-D" reach the program
# instead of being interpreted by PowerShell.
function Invoke-Native {
    param(
        [Parameter(Mandatory)][string]$Command,
        [string[]]$Arguments = @()
    )
    Write-Host "> $Command $($Arguments -join ' ')" -ForegroundColor Cyan
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command exited with code $LASTEXITCODE"
    }
}

# Runs a native command and returns its trimmed standard output.
function Get-NativeOutput {
    param(
        [Parameter(Mandatory)][string]$Command,
        [string[]]$Arguments = @()
    )
    $output = & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command $($Arguments -join ' ') exited with code $LASTEXITCODE"
    }
    ($output -join "`n").Trim()
}

# Returns the repository root, one level above scripts/.
function Get-RepositoryRoot {
    Split-Path -Parent $PSScriptRoot
}

# Reads the package version from Cargo.toml.
function Get-CargoVersion {
    param([Parameter(Mandatory)][string]$Root)
    $text = [System.IO.File]::ReadAllText((Join-Path $Root 'Cargo.toml'))
    $match = [regex]::Match($text, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $match.Success) {
        throw 'No package version found in Cargo.toml'
    }
    $match.Groups[1].Value
}
