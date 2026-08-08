[CmdletBinding()]
param(
    [string] $ProjectRoot,
    [string] $TargetDirectory,
    [Parameter(Mandatory = $true)]
    [string] $SdlDll,
    [string] $OutputArchive,
    [switch] $SkipSmoke,
    [switch] $UiSmoke
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw "PowerShell 7 or newer is required; run this script with pwsh."
}

if ([string]::IsNullOrWhiteSpace($ProjectRoot)) {
    $ProjectRoot = Join-Path $PSScriptRoot ".."
}
$ProjectRoot = [System.IO.Path]::GetFullPath($ProjectRoot)
if ([string]::IsNullOrWhiteSpace($TargetDirectory)) {
    $TargetDirectory = Join-Path $ProjectRoot "target/x86_64-pc-windows-msvc/release"
}
$TargetDirectory = [System.IO.Path]::GetFullPath($TargetDirectory)
$SdlDll = [System.IO.Path]::GetFullPath($SdlDll)
if ([string]::IsNullOrWhiteSpace($OutputArchive)) {
    $OutputArchive = Join-Path $ProjectRoot "dist/cockroach-overlay-windows-x64.zip"
}
$OutputArchive = [System.IO.Path]::GetFullPath($OutputArchive)

function Assert-File {
    param([Parameter(Mandatory = $true)][string] $Path)
    if (-not [System.IO.File]::Exists($Path)) {
        throw "Required file is missing: $Path"
    }
}

function Copy-PayloadFile {
    param(
        [Parameter(Mandatory = $true)][string] $Source,
        [Parameter(Mandatory = $true)][string] $RelativeDestination,
        [Parameter(Mandatory = $true)][string] $PayloadRoot
    )
    Assert-File $Source
    $destination = Join-Path $PayloadRoot $RelativeDestination
    $parent = Split-Path -Parent $destination
    [System.IO.Directory]::CreateDirectory($parent) | Out-Null
    [System.IO.File]::Copy($Source, $destination, $true)
}

function Copy-PayloadTree {
    param(
        [Parameter(Mandatory = $true)][string] $SourceRoot,
        [Parameter(Mandatory = $true)][string] $RelativeDestination,
        [Parameter(Mandatory = $true)][string] $PayloadRoot
    )
    if (-not [System.IO.Directory]::Exists($SourceRoot)) {
        throw "Required directory is missing: $SourceRoot"
    }
    $source = (Resolve-Path -LiteralPath $SourceRoot).Path
    $entries = @(Get-ChildItem -LiteralPath $source -Force -Recurse)
    foreach ($entry in $entries) {
        if (($entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Refusing to package reparse point: $($entry.FullName)"
        }
        $relative = [System.IO.Path]::GetRelativePath($source, $entry.FullName)
        $destination = Join-Path (Join-Path $PayloadRoot $RelativeDestination) $relative
        if ($entry.PSIsContainer) {
            [System.IO.Directory]::CreateDirectory($destination) | Out-Null
        } else {
            [System.IO.Directory]::CreateDirectory((Split-Path -Parent $destination)) | Out-Null
            [System.IO.File]::Copy($entry.FullName, $destination, $true)
        }
    }
}

function Get-OrdinalFiles {
    param([Parameter(Mandatory = $true)][string] $Root)
    [string[]] $paths = @(
        Get-ChildItem -LiteralPath $Root -File -Force -Recurse |
            ForEach-Object { $_.FullName }
    )
    [System.Array]::Sort($paths, [System.StringComparer]::Ordinal)
    return $paths
}

function Write-PayloadHashes {
    param([Parameter(Mandatory = $true)][string] $PayloadRoot)
    $lines = [System.Collections.Generic.List[string]]::new()
    foreach ($file in (Get-OrdinalFiles $PayloadRoot)) {
        if ([System.IO.Path]::GetFileName($file) -eq "SHA256SUMS.txt") {
            continue
        }
        $relative = [System.IO.Path]::GetRelativePath($PayloadRoot, $file).Replace("\", "/")
        $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
        $lines.Add("$hash  $relative")
    }
    $utf8WithoutBom = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllLines(
        (Join-Path $PayloadRoot "SHA256SUMS.txt"),
        $lines,
        $utf8WithoutBom
    )
}

function Write-DeterministicZip {
    param(
        [Parameter(Mandatory = $true)][string] $StageRoot,
        [Parameter(Mandatory = $true)][string] $Archive
    )
    Add-Type -AssemblyName System.IO.Compression
    $archiveParent = Split-Path -Parent $Archive
    [System.IO.Directory]::CreateDirectory($archiveParent) | Out-Null
    if ([System.IO.File]::Exists($Archive)) {
        [System.IO.File]::Delete($Archive)
    }

    $stream = [System.IO.File]::Open(
        $Archive,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::None
    )
    try {
        $zip = [System.IO.Compression.ZipArchive]::new(
            $stream,
            [System.IO.Compression.ZipArchiveMode]::Create,
            $false
        )
        try {
            $timestamp = [System.DateTimeOffset]::new(
                1980, 1, 1, 0, 0, 0, [System.TimeSpan]::Zero
            )
            foreach ($file in (Get-OrdinalFiles $StageRoot)) {
                $entryName = [System.IO.Path]::GetRelativePath($StageRoot, $file).Replace("\", "/")
                $entry = $zip.CreateEntry(
                    $entryName,
                    [System.IO.Compression.CompressionLevel]::Optimal
                )
                $entry.LastWriteTime = $timestamp
                $input = [System.IO.File]::OpenRead($file)
                $output = $entry.Open()
                try {
                    $input.CopyTo($output)
                } finally {
                    $output.Dispose()
                    $input.Dispose()
                }
            }
        } finally {
            $zip.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
}

function Invoke-Smoke {
    param(
        [Parameter(Mandatory = $true)][string] $Executable,
        [Parameter(Mandatory = $true)][string[]] $Arguments,
        [Parameter(Mandatory = $true)][string] $WorkingDirectory,
        [int] $TimeoutMilliseconds = 15000
    )
    $process = Start-Process -FilePath $Executable `
        -ArgumentList $Arguments `
        -WorkingDirectory $WorkingDirectory `
        -PassThru
    if (-not $process.WaitForExit($TimeoutMilliseconds)) {
        $process.Kill($true)
        throw "Smoke process timed out: $Executable $($Arguments -join ' ')"
    }
    if ($process.ExitCode -ne 0) {
        throw "Smoke process exited with code $($process.ExitCode): $Executable"
    }
}

$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "bug-overlay-package-" + [System.Guid]::NewGuid().ToString("N")
)
$payloadRoot = Join-Path $temporaryRoot "windows-x64"
$smokeWorkingDirectory = Join-Path $temporaryRoot "unrelated working directory"

try {
    [System.IO.Directory]::CreateDirectory($payloadRoot) | Out-Null
    [System.IO.Directory]::CreateDirectory($smokeWorkingDirectory) | Out-Null

    Copy-PayloadFile `
        (Join-Path $TargetDirectory "cockroach_overlay.exe") `
        "cockroach_overlay.exe" $payloadRoot
    Copy-PayloadFile `
        (Join-Path $TargetDirectory "cockroach_swarm_20.exe") `
        "cockroach_swarm_20.exe" $payloadRoot
    Copy-PayloadFile `
        (Join-Path $TargetDirectory "turtle_overlay.exe") `
        "turtle_overlay.exe" $payloadRoot
    Copy-PayloadFile $SdlDll "SDL2.dll" $payloadRoot
    Copy-PayloadFile `
        (Join-Path $ProjectRoot "packaging/WINDOWS-README.txt") `
        "README.txt" $payloadRoot
    Copy-PayloadFile `
        (Join-Path $ProjectRoot "LICENSE") `
        "LICENSE" $payloadRoot
    Copy-PayloadFile `
        (Join-Path $ProjectRoot "ASSET-NOTICE.md") `
        "ASSET-NOTICE.md" $payloadRoot
    Copy-PayloadFile `
        (Join-Path $ProjectRoot "packaging/THIRD_PARTY_LICENSES.txt") `
        "THIRD_PARTY_LICENSES.txt" $payloadRoot

    foreach ($package in @("runtime", "cockroach", "turtle", "template")) {
        Copy-PayloadTree `
            (Join-Path $ProjectRoot "bugs/$package") `
            "bugs/$package" `
            $payloadRoot
    }

    Write-PayloadHashes $payloadRoot

    if (-not $SkipSmoke) {
        Invoke-Smoke `
            (Join-Path $payloadRoot "cockroach_overlay.exe") `
            @("--help") `
            $smokeWorkingDirectory
        Invoke-Smoke `
            (Join-Path $payloadRoot "cockroach_swarm_20.exe") `
            @("--help") `
            $smokeWorkingDirectory
        Invoke-Smoke `
            (Join-Path $payloadRoot "turtle_overlay.exe") `
            @("--help") `
            $smokeWorkingDirectory
    }
    if ($UiSmoke) {
        Invoke-Smoke `
            (Join-Path $payloadRoot "cockroach_overlay.exe") `
            @("--frames", "3", "--seed", "1") `
            $smokeWorkingDirectory `
            30000
        Invoke-Smoke `
            (Join-Path $payloadRoot "cockroach_swarm_20.exe") `
            @("--frames", "3", "--seed", "1") `
            $smokeWorkingDirectory `
            30000
        Invoke-Smoke `
            (Join-Path $payloadRoot "turtle_overlay.exe") `
            @("--frames", "3", "--seed", "1") `
            $smokeWorkingDirectory `
            30000
    }

    Write-DeterministicZip $temporaryRoot $OutputArchive
    $archiveHash = (Get-FileHash -LiteralPath $OutputArchive -Algorithm SHA256).Hash.ToLowerInvariant()
    $utf8WithoutBom = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText(
        "$OutputArchive.sha256",
        "$archiveHash  $([System.IO.Path]::GetFileName($OutputArchive))`n",
        $utf8WithoutBom
    )

    Write-Host "Windows x64 package: $OutputArchive"
    Write-Host "SHA-256: $archiveHash"
} finally {
    if ([System.IO.Directory]::Exists($temporaryRoot)) {
        [System.IO.Directory]::Delete($temporaryRoot, $true)
    }
}
