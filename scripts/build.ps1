# Builds me-voice on Windows inside the Visual Studio build environment,
# which whisper.cpp's bindings generator needs to find the C headers.
#
#   .\scripts\build.ps1            debug build
#   .\scripts\build.ps1 -Release   optimised build (target\release\me-voice.exe)
#   .\scripts\build.ps1 -Test      run the unit tests
#   .\scripts\build.ps1 -Installer release build + installer (installer\output\me-voice-setup-<version>.exe)
param([switch]$Release, [switch]$Test, [switch]$Installer)

$ErrorActionPreference = 'Stop'
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Visual Studio C++ build tools not found.' }
$vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'

# Copy the variables vcvars64.bat sets into this PowerShell session.
cmd /c "`"$vcvars`" >nul 2>&1 && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "env:$($Matches[1])" -Value $Matches[2] }
}
$env:Path += ";$env:USERPROFILE\.cargo\bin"

# libclang doesn't always detect new Visual Studio versions, so pass the header folders explicitly.
$clangInclude = Get-ChildItem 'C:\Program Files\LLVM\lib\clang' -Directory | Sort-Object Name | Select-Object -Last 1
$includes = @("$($clangInclude.FullName)\include") + ($env:INCLUDE -split ';' | Where-Object { $_ })
$env:BINDGEN_EXTRA_CLANG_ARGS = ($includes | ForEach-Object { "`"-isystem$($_.TrimEnd('\') -replace '\\', '/')`"" }) -join ' '

# whisper-rs-sys builds whisper.cpp with CMake's "Release" config, but the cmake crate
# replaces CMake's release compiler flags with its own, which on MSVC drop /O2. The
# result was an unoptimised speech engine: 5-7x slower transcription in release builds.
# Setting the flags here makes the cmake crate keep them.
$env:CMAKE_C_FLAGS_RELEASE = '/MD /O2 /Ob2 /DNDEBUG'
$env:CMAKE_CXX_FLAGS_RELEASE = '/MD /O2 /Ob2 /DNDEBUG'

# whisper.cpp is tuned to the build machine's CPU by default. An installer runs on other
# computers: a GitHub build used AVX-512 and crashed with an illegal instruction on CPUs
# without it. So installers target AVX2 (Intel 2013+, AMD 2015+) instead. whisper-rs-sys
# passes GGML_* variables on to CMake but doesn't rebuild when they change, so its old
# build is cleaned first (below).
if ($Installer) {
    $env:GGML_NATIVE = 'OFF'
    $env:GGML_AVX = 'ON'
    $env:GGML_AVX2 = 'ON'
    $env:GGML_FMA = 'ON'
    $env:GGML_F16C = 'ON'
}

$root = Split-Path $PSScriptRoot
Set-Location $root
if ($Test) { cargo test; exit $LASTEXITCODE }
if ($Installer) { cargo clean --release -p whisper-rs-sys }
if ($Release -or $Installer) { cargo build --release } else { cargo build }
if ($LASTEXITCODE -ne 0 -or -not $Installer) { exit $LASTEXITCODE }

# Installer: needs Inno Setup (winget install --id JRSoftware.InnoSetup -e).
$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup not found. Install it with: winget install --id JRSoftware.InnoSetup -e' }
$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"').Matches[0].Groups[1].Value
New-Item -ItemType Directory -Force installer\build | Out-Null
# The release exe is a GUI app, so PowerShell would not wait for it without -Wait.
Start-Process .\target\release\me-voice.exe -ArgumentList 'write-icon', 'installer\build\me-voice.ico' -Wait
# Inno Setup stamps the icon into the setup exe right after writing it. Antivirus
# scanning that fresh 10 MB executable can hold it open just long enough to fail
# with "EndUpdateResource failed (110)", so give it a few tries.
$setup = "installer\output\me-voice-setup-$version.exe"
foreach ($attempt in 1..3) {
    & $iscc /Q "/DAppVersion=$version" installer\me-voice.iss
    if ($LASTEXITCODE -eq 0) { break }
    if ($attempt -eq 3) { exit $LASTEXITCODE }
    Write-Host "Inno Setup failed (attempt $attempt of 3), retrying..."
    Remove-Item $setup -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
}
exit $LASTEXITCODE
