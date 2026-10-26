# Bumps the version and writes release notes from the git log.
#
#   .\scripts\release.ps1 -Bump minor   bump Cargo.toml + Cargo.lock, add a section to CHANGELOG.md
#   .\scripts\release.ps1 -Notes        print the notes for the commits since the last tag (changes nothing)
#
# -Bump takes major, minor or patch. Notes cover the commits since the latest v* tag (all commits
# if there is none), so tag each release (git tag v0.8.0) to keep the next one short.
# Review CHANGELOG.md afterwards; commit subjects are only a starting point. Nothing is committed.
param([ValidateSet('major', 'minor', 'patch')][string]$Bump, [switch]$Notes)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
Set-Location $root
if (-not $Bump -and -not $Notes) { throw 'Pass -Bump major|minor|patch or -Notes.' }

function Get-Notes {
    $last = git describe --tags --abbrev=0 --match 'v*' 2>$null
    $range = if ($last) { "$last..HEAD" } else { 'HEAD' }
    $subjects = git log $range --no-merges --pretty='format:%s' | Where-Object { $_ -and $_ -ne 'init' }
    if (-not $subjects) { return '- No changes.' }
    ($subjects | ForEach-Object { "- $_" }) -join "`n"
}

$body = Get-Notes
if ($Notes) { $body; exit 0 }

$toml = Get-Content Cargo.toml -Raw
$m = [regex]::Match($toml, '(?m)^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"')
if (-not $m.Success) { throw 'No version found in Cargo.toml.' }
$major, $minor, $patch = [int]$m.Groups[1].Value, [int]$m.Groups[2].Value, [int]$m.Groups[3].Value
switch ($Bump) {
    'major' { $major++; $minor = 0; $patch = 0 }
    'minor' { $minor++; $patch = 0 }
    'patch' { $patch++ }
}
$new = "$major.$minor.$patch"
$old = "$($m.Groups[1].Value).$($m.Groups[2].Value).$($m.Groups[3].Value)"

# Cargo.toml: only the first match, which is the [package] version.
$toml = [regex]::new('(?m)^version\s*=\s*"[^"]+"').Replace($toml, "version = `"$new`"", 1)
[IO.File]::WriteAllText("$root\Cargo.toml", $toml)

# Cargo.lock: the me-voice package entry.
$lock = Get-Content Cargo.lock -Raw
$lock = $lock -replace '(?m)(name = "me-voice"\r?\nversion = )"[^"]+"', "`$1`"$new`""
[IO.File]::WriteAllText("$root\Cargo.lock", $lock)

# CHANGELOG.md: newest section goes right under the title.
$section = "## $new - $(Get-Date -Format 'yyyy-MM-dd')`n`n$body`n"
$path = "$root\CHANGELOG.md"
if (Test-Path $path) {
    $text = Get-Content $path -Raw
    $first = [regex]::Match($text, '(?m)^## ')
    $text = if ($first.Success) { $text.Insert($first.Index, "$section`n") } else { $text.TrimEnd() + "`n`n$section" }
} else {
    $text = "# Changelog`n`n$section"
}
[IO.File]::WriteAllText($path, $text)

Write-Host "Bumped $old -> $new. Notes added to CHANGELOG.md; review them, then commit and tag v$new."
