<#
.SYNOPSIS
Publishes a release: bumps the version, tags it and pushes.

.DESCRIPTION
Checks that the working tree is clean and up to date on main and that the tag
is unused, sets the version in Cargo.toml, runs scripts\build.ps1 (checks
included), commits "Release vX.Y.Z", creates the annotated tag vX.Y.Z and
pushes both. The push of the tag makes the GitHub Actions workflow build the
executable and create the GitHub release.

.PARAMETER Version
The new version, as X.Y.Z. It must not be lower than the current one; the
same version is fine for a first release. Release notes must exist in
docs\release-notes\vX.Y.Z.md.

.PARAMETER DryRun
Only check the preconditions and print what would happen.

.PARAMETER Yes
Do not ask for confirmation before committing and pushing.

.EXAMPLE
.\scripts\release.ps1 -Version 0.2.0 -DryRun
.\scripts\release.ps1 -Version 0.2.0
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version,
    [switch]$DryRun,
    [switch]$Yes
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'common.ps1')

$Branch = 'main'
$Remote = 'origin'
$tag = "v$Version"
$root = Get-RepositoryRoot

function Assert-ReleasePreconditions {
    param([Parameter(Mandatory)][string]$current)
    if ([version]$Version -lt [version]$current) {
        throw "Version $Version must not be lower than the current version $current"
    }
    $notes = Get-ReleaseNotesPath $root $tag
    if (-not (Test-Path $notes) -or -not (Get-Content $notes -Raw).Trim()) {
        throw "Write the release notes first: $notes"
    }

    $onBranch = Get-NativeOutput git @('rev-parse', '--abbrev-ref', 'HEAD')
    if ($onBranch -ne $Branch) {
        throw "Releases are made from '$Branch', but the current branch is '$onBranch'"
    }
    if (Get-NativeOutput git @('status', '--porcelain')) {
        throw 'The working tree has uncommitted changes; commit or stash them first'
    }

    Invoke-Native git @('fetch', $Remote, '--tags')
    & git merge-base --is-ancestor "$Remote/$Branch" HEAD
    if ($LASTEXITCODE -ne 0) {
        throw "Local $Branch is behind $Remote/$Branch; pull first"
    }
    if (Get-NativeOutput git @('tag', '--list', $tag)) {
        throw "Tag $tag already exists locally"
    }
    if (Get-NativeOutput git @('ls-remote', '--tags', $Remote, "refs/tags/$tag")) {
        throw "Tag $tag already exists on $Remote"
    }
}

function Set-CargoVersion {
    $path = Join-Path $root 'Cargo.toml'
    $text = [System.IO.File]::ReadAllText($path)
    $pattern = [regex]'(?m)^version\s*=\s*"[^"]+"'
    $updated = $pattern.Replace($text, "version = `"$Version`"", 1)
    if ($updated -eq $text) {
        throw 'Cargo.toml was not changed'
    }
    [System.IO.File]::WriteAllText($path, $updated)
}

Push-Location $root
try {
    $current = Get-CargoVersion $root
    Assert-ReleasePreconditions $current
    Write-Host "Release $tag (current version $current)" -ForegroundColor Green

    if ($DryRun) {
        Write-Host 'Dry run: preconditions hold. A real run would:'
        Write-Host "  1. set version = `"$Version`" in Cargo.toml (if it differs)"
        Write-Host '  2. run scripts\build.ps1 (fmt, clippy, tests, release build)'
        Write-Host "  3. commit 'Release $tag' (if the version changed) and create the annotated tag $tag"
        Write-Host "  4. push $Branch and $tag to $Remote"
        return
    }

    if (-not $Yes) {
        $answer = Read-Host "Type '$tag' to commit, tag and push"
        if ($answer -ne $tag) {
            throw 'Cancelled'
        }
    }

    try {
        if ($Version -ne $current) {
            Set-CargoVersion
        }
        & (Join-Path $PSScriptRoot 'build.ps1')
    }
    catch {
        # The tree was clean, so restoring these two files undoes only our edit.
        & git checkout -- Cargo.toml Cargo.lock
        throw
    }

    Invoke-Native git @('add', 'Cargo.toml', 'Cargo.lock')
    if (Get-NativeOutput git @('diff', '--cached', '--name-only')) {
        Invoke-Native git @('commit', '-m', "Release $tag")
    }
    Invoke-Native git @('tag', '-a', $tag, '-m', "Release $tag")
    Invoke-Native git @('push', '--atomic', $Remote, $Branch, $tag)

    $url = (Get-NativeOutput git @('remote', 'get-url', $Remote)) -replace '\.git$', ''
    Write-Host ''
    Write-Host "Pushed $tag. The release is created by the workflow:" -ForegroundColor Green
    Write-Host "  $url/actions"
    Write-Host "  $url/releases/tag/$tag"
}
finally {
    Pop-Location
}
