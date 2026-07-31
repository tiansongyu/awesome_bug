[CmdletBinding()]
param(
    [switch] $SkipTests,
    [switch] $SkipSmoke,
    [switch] $UiSmoke
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw "PowerShell 7 or newer is required; run this script with pwsh."
}

$projectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$dependencyRoot = Join-Path $projectRoot "build-windows-deps/msvc"
$downloadRoot = Join-Path $projectRoot "build-windows-deps/downloads"
$target = "x86_64-pc-windows-msvc"
$sdlVersion = "2.32.10"
$sdlArchiveName = "SDL2-devel-$sdlVersion-VC.zip"
$sdlArchiveHash = "af347939395a58b365846aaea27391e69f9ec9d4dd650d6ac40802159b418a6e"
$sdlUrl = "https://github.com/libsdl-org/SDL/releases/download/release-$sdlVersion/$sdlArchiveName"
$sdlArchive = Join-Path $downloadRoot $sdlArchiveName
$sdlRoot = Join-Path $dependencyRoot "SDL2-$sdlVersion"

function Invoke-Native {
    param(
        [Parameter(Mandatory = $true)][string] $Program,
        [string[]] $ArgumentList = @()
    )
    & $Program @ArgumentList
    if ($LASTEXITCODE -ne 0) {
        throw "$Program exited with code $LASTEXITCODE"
    }
}

[System.IO.Directory]::CreateDirectory($downloadRoot) | Out-Null
[System.IO.Directory]::CreateDirectory($dependencyRoot) | Out-Null

if (-not [System.IO.File]::Exists($sdlArchive)) {
    $temporaryArchive = "$sdlArchive.download.$PID"
    try {
        Invoke-WebRequest `
            -Uri $sdlUrl `
            -OutFile $temporaryArchive `
            -UseBasicParsing `
            -MaximumRetryCount 3 `
            -RetryIntervalSec 2
        $downloadedHash = (Get-FileHash -LiteralPath $temporaryArchive -Algorithm SHA256).Hash
        if ($downloadedHash -ne $sdlArchiveHash) {
            throw "SDL2 archive hash mismatch: expected $sdlArchiveHash, got $downloadedHash"
        }
        Move-Item -LiteralPath $temporaryArchive -Destination $sdlArchive
    } finally {
        if ([System.IO.File]::Exists($temporaryArchive)) {
            [System.IO.File]::Delete($temporaryArchive)
        }
    }
}

$actualSdlHash = (Get-FileHash -LiteralPath $sdlArchive -Algorithm SHA256).Hash
if ($actualSdlHash -ne $sdlArchiveHash) {
    throw "Cached SDL2 archive hash mismatch: expected $sdlArchiveHash, got $actualSdlHash"
}

$sdlLibrary = Join-Path $sdlRoot "lib/x64/SDL2.lib"
$sdlDll = Join-Path $sdlRoot "lib/x64/SDL2.dll"
$sdlHeader = Join-Path $sdlRoot "include/SDL.h"
if (
    -not [System.IO.File]::Exists($sdlLibrary) -or
    -not [System.IO.File]::Exists($sdlDll) -or
    -not [System.IO.File]::Exists($sdlHeader)
) {
    Expand-Archive -LiteralPath $sdlArchive -DestinationPath $dependencyRoot -Force
}
foreach ($requiredPath in @($sdlLibrary, $sdlDll, $sdlHeader)) {
    if (-not [System.IO.File]::Exists($requiredPath)) {
        throw "Incomplete SDL2 $sdlVersion tree: $requiredPath is missing"
    }
}

$rustVersion = (& rustc --version)
if ($LASTEXITCODE -ne 0 -or $rustVersion -notmatch '^rustc 1\.97\.1 ') {
    throw "Rust 1.97.1 is required; active toolchain: $rustVersion"
}
$installedTargets = @(& rustup target list --installed)
if ($LASTEXITCODE -ne 0 -or $installedTargets -notcontains $target) {
    throw "Missing Rust target $target; run: rustup target add $target --toolchain 1.97.1"
}

$env:SDL2_LIB_DIR = Join-Path $sdlRoot "lib/x64"
$env:SDL2_INCLUDE_PATH = Join-Path $sdlRoot "include"
# Cargo test executables import SDL2.dll even when an individual pure test
# never calls SDL. Put the verified DLL directory on the loader path for the
# native MSVC test phase; packaged applications still use the adjacent copy.
$env:PATH = "$env:SDL2_LIB_DIR;$env:PATH"
$targetRustFlagsName = "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS"
$existingTargetRustFlags = [System.Environment]::GetEnvironmentVariable(
    $targetRustFlagsName
)
$targetRustFlags = (
    "$existingTargetRustFlags -C target-feature=+crt-static"
).Trim()
[System.Environment]::SetEnvironmentVariable(
    $targetRustFlagsName,
    $targetRustFlags,
    [System.EnvironmentVariableTarget]::Process
)

Push-Location $projectRoot
try {
    if (-not $SkipTests) {
        Invoke-Native "cargo" @(
            "clippy",
            "-p", "bug-windows",
            "--all-targets",
            "--target", $target,
            "--locked",
            "--",
            "-D", "warnings"
        )
        Invoke-Native "cargo" @(
            "test",
            "-p", "bug-runtime",
            "-p", "bug-windows",
            "--all-targets",
            "--target", $target,
            "--locked"
        )
    }
    Invoke-Native "cargo" @(
        "build",
        "-p", "bug-windows",
        "--bins",
        "--release",
        "--target", $target,
        "--locked"
    )
} finally {
    Pop-Location
}

$packageArguments = @{
    ProjectRoot = $projectRoot
    TargetDirectory = (Join-Path $projectRoot "target/$target/release")
    SdlDll = $sdlDll
}
if ($SkipSmoke) {
    $packageArguments.SkipSmoke = $true
}
if ($UiSmoke) {
    $packageArguments.UiSmoke = $true
}
& (Join-Path $PSScriptRoot "package-windows.ps1") @packageArguments
