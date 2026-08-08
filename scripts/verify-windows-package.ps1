[CmdletBinding()]
param(
    [string] $ProjectRoot,
    [string] $Archive,
    [switch] $SkipIconProbe
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
if ([string]::IsNullOrWhiteSpace($Archive)) {
    $Archive = Join-Path $ProjectRoot "dist/cockroach-overlay-windows-x64.zip"
}
$Archive = [System.IO.Path]::GetFullPath($Archive)
$sidecar = "$Archive.sha256"

function Assert-File {
    param([Parameter(Mandatory = $true)][string] $Path)
    if (-not [System.IO.File]::Exists($Path)) {
        throw "Required file is missing: $Path"
    }
    if ((Get-Item -LiteralPath $Path).Length -eq 0) {
        throw "Required file is empty: $Path"
    }
}

function Get-RelativeFiles {
    param([Parameter(Mandatory = $true)][string] $Root)
    [string[]] $paths = @(
        Get-ChildItem -LiteralPath $Root -File -Force -Recurse |
            ForEach-Object {
                [System.IO.Path]::GetRelativePath(
                    $Root,
                    $_.FullName
                ).Replace("\", "/")
            }
    )
    [System.Array]::Sort($paths, [System.StringComparer]::Ordinal)
    return $paths
}

function Assert-SameFile {
    param(
        [Parameter(Mandatory = $true)][string] $Expected,
        [Parameter(Mandatory = $true)][string] $Actual
    )
    Assert-File $Expected
    Assert-File $Actual
    $expectedHash = (Get-FileHash -LiteralPath $Expected -Algorithm SHA256).Hash
    $actualHash = (Get-FileHash -LiteralPath $Actual -Algorithm SHA256).Hash
    if ($expectedHash -ne $actualHash) {
        throw "Packaged file differs from source: $Actual"
    }
}

function Assert-SameTree {
    param(
        [Parameter(Mandatory = $true)][string] $ExpectedRoot,
        [Parameter(Mandatory = $true)][string] $ActualRoot
    )
    if (-not [System.IO.Directory]::Exists($ExpectedRoot)) {
        throw "Required source directory is missing: $ExpectedRoot"
    }
    if (-not [System.IO.Directory]::Exists($ActualRoot)) {
        throw "Required package directory is missing: $ActualRoot"
    }
    $expectedFiles = @(Get-RelativeFiles $ExpectedRoot)
    $actualFiles = @(Get-RelativeFiles $ActualRoot)
    if (Compare-Object $expectedFiles $actualFiles) {
        throw "Packaged tree differs from source tree: $ActualRoot"
    }
    foreach ($relative in $expectedFiles) {
        Assert-SameFile `
            (Join-Path $ExpectedRoot $relative) `
            (Join-Path $ActualRoot $relative)
    }
}

function Assert-WindowsGuiPe {
    param([Parameter(Mandatory = $true)][string] $Path)

    [byte[]] $bytes = [System.IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 512) {
        throw "PE file is unexpectedly small: $Path"
    }
    $peOffset = [System.BitConverter]::ToInt32($bytes, 0x3c)
    if (
        $peOffset -lt 0 -or
        $peOffset + 24 -ge $bytes.Length -or
        $bytes[$peOffset] -ne 0x50 -or
        $bytes[$peOffset + 1] -ne 0x45 -or
        $bytes[$peOffset + 2] -ne 0 -or
        $bytes[$peOffset + 3] -ne 0
    ) {
        throw "Invalid PE signature: $Path"
    }

    $machine = [System.BitConverter]::ToUInt16($bytes, $peOffset + 4)
    $sectionCount = [System.BitConverter]::ToUInt16($bytes, $peOffset + 6)
    $optionalSize = [System.BitConverter]::ToUInt16($bytes, $peOffset + 20)
    $optionalOffset = $peOffset + 24
    $magic = [System.BitConverter]::ToUInt16($bytes, $optionalOffset)
    $subsystem = [System.BitConverter]::ToUInt16(
        $bytes,
        $optionalOffset + 68
    )
    if ($machine -ne 0x8664 -or $magic -ne 0x20b) {
        throw "Executable is not PE32+ x86-64: $Path"
    }
    if ($subsystem -ne 2) {
        throw "Executable does not use the Windows GUI subsystem: $Path"
    }

    $sectionOffset = $optionalOffset + $optionalSize
    $sectionNames = @()
    $resourceOffset = $null
    for ($index = 0; $index -lt $sectionCount; $index++) {
        $offset = $sectionOffset + 40 * $index
        if ($offset + 40 -gt $bytes.Length) {
            throw "Truncated PE section table: $Path"
        }
        $name = (
            [System.Text.Encoding]::ASCII.GetString(
                $bytes,
                $offset,
                8
            ).Trim([char] 0)
        )
        $sectionNames += $name
        if ($name -eq ".rsrc") {
            $resourceOffset = [System.BitConverter]::ToUInt32(
                $bytes,
                $offset + 20
            )
        }
    }
    if ($sectionNames -notcontains ".rsrc" -or $null -eq $resourceOffset) {
        throw "Executable has no resource section: $Path"
    }
    if ($resourceOffset + 16 -gt $bytes.Length) {
        throw "Executable has a truncated resource directory: $Path"
    }
    $namedResources = [System.BitConverter]::ToUInt16(
        $bytes,
        $resourceOffset + 12
    )
    $idResources = [System.BitConverter]::ToUInt16(
        $bytes,
        $resourceOffset + 14
    )
    $resourceTypes = @()
    for (
        $index = 0;
        $index -lt $namedResources + $idResources;
        $index++
    ) {
        $entryOffset = $resourceOffset + 16 + 8 * $index
        if ($entryOffset + 8 -gt $bytes.Length) {
            throw "Executable has a truncated resource type table: $Path"
        }
        $resourceId = [System.BitConverter]::ToUInt32($bytes, $entryOffset)
        if (($resourceId -band 0x80000000) -eq 0) {
            $resourceTypes += [int] $resourceId
        }
    }
    foreach ($requiredResourceType in @(3, 14, 16, 24)) {
        if ($resourceTypes -notcontains $requiredResourceType) {
            throw "Executable is missing resource type $requiredResourceType`: $Path"
        }
    }

    if ($IsWindows) {
        $version = (Get-Item -LiteralPath $Path).VersionInfo
        if (
            $version.FileDescription -ne
                "Scriptable Bug Overlay (Rust + Lua)" -or
            $version.ProductName -ne "Scriptable Bug Overlay"
        ) {
            throw "Executable version resource is missing or unexpected: $Path"
        }
    }
}

Assert-File $Archive
Assert-File $sidecar
$expectedArchiveHash = (
    Get-Content -LiteralPath $sidecar -Raw
).Split(" ", [System.StringSplitOptions]::RemoveEmptyEntries)[0]
$actualArchiveHash = (
    Get-FileHash -LiteralPath $Archive -Algorithm SHA256
).Hash.ToLowerInvariant()
if ($actualArchiveHash -ne $expectedArchiveHash) {
    throw "ZIP SHA-256 mismatch"
}

Add-Type -AssemblyName System.IO.Compression
$zip = [System.IO.Compression.ZipFile]::OpenRead($Archive)
try {
    $entryNames = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($entry in $zip.Entries) {
        $name = $entry.FullName
        if (
            $name.Contains("\") -or
            -not $name.StartsWith("windows-x64/") -or
            $name.EndsWith("/") -or
            $name.Split("/") -contains ".." -or
            -not $entryNames.Add($name)
        ) {
            throw "Unsafe or duplicate ZIP entry: $name"
        }
    }
} finally {
    $zip.Dispose()
}

$extractRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "cockroach-verify-" + [System.Guid]::NewGuid().ToString("N")
)
try {
    Expand-Archive -LiteralPath $Archive -DestinationPath $extractRoot
    $payload = Join-Path $extractRoot "windows-x64"

    $required = @(
        "cockroach_overlay.exe",
        "cockroach_swarm_20.exe",
        "turtle_overlay.exe",
        "SDL2.dll",
        "README.txt",
        "LICENSE",
        "ASSET-NOTICE.md",
        "THIRD_PARTY_LICENSES.txt",
        "SHA256SUMS.txt",
        "bugs/runtime/fsm.lua",
        "bugs/cockroach/manifest.lua",
        "bugs/cockroach/behavior.lua",
        "bugs/cockroach/cockroach_parts_atlas.png",
        "bugs/turtle/manifest.lua",
        "bugs/turtle/behavior.lua",
        "bugs/turtle/turtle_parts_atlas.png",
        "bugs/turtle/ARTWORK.md",
        "bugs/template/manifest.lua",
        "bugs/template/behavior.lua",
        "bugs/template/atlas.png",
        "bugs/template/README.md"
    )
    foreach ($relative in $required) {
        Assert-File (Join-Path $payload $relative)
    }

    [string[]] $dlls = @(
        Get-ChildItem -LiteralPath $payload -File -Recurse -Filter "*.dll" |
            ForEach-Object {
                [System.IO.Path]::GetRelativePath(
                    $payload,
                    $_.FullName
                ).Replace("\", "/")
            }
    )
    if ($dlls.Count -ne 1 -or $dlls[0] -ne "SDL2.dll") {
        throw "Package must contain only SDL2.dll; found: $($dlls -join ', ')"
    }

    Assert-SameFile `
        (Join-Path $ProjectRoot "LICENSE") `
        (Join-Path $payload "LICENSE")
    Assert-SameFile `
        (Join-Path $ProjectRoot "ASSET-NOTICE.md") `
        (Join-Path $payload "ASSET-NOTICE.md")
    Assert-SameFile `
        (Join-Path $ProjectRoot "packaging/WINDOWS-README.txt") `
        (Join-Path $payload "README.txt")
    Assert-SameFile `
        (Join-Path $ProjectRoot "packaging/THIRD_PARTY_LICENSES.txt") `
        (Join-Path $payload "THIRD_PARTY_LICENSES.txt")
    foreach ($packageName in @("runtime", "cockroach", "turtle", "template")) {
        Assert-SameTree `
            (Join-Path $ProjectRoot "bugs/$packageName") `
            (Join-Path $payload "bugs/$packageName")
    }

    $payloadPrefix = [System.IO.Path]::GetFullPath($payload) +
        [System.IO.Path]::DirectorySeparatorChar
    $hashedFiles = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($line in (Get-Content -LiteralPath (
        Join-Path $payload "SHA256SUMS.txt"
    ))) {
        if ($line -notmatch '^([0-9a-f]{64})  ([^\\]+)$') {
            throw "Malformed SHA256SUMS.txt line: $line"
        }
        $relative = $Matches[2]
        if (
            $relative -eq "SHA256SUMS.txt" -or
            -not $hashedFiles.Add($relative)
        ) {
            throw "Duplicate or recursive payload hash: $relative"
        }
        $file = [System.IO.Path]::GetFullPath((Join-Path $payload $relative))
        if (-not $file.StartsWith(
            $payloadPrefix,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
            throw "Unsafe path in SHA256SUMS.txt: $relative"
        }
        $actual = (
            Get-FileHash -LiteralPath $file -Algorithm SHA256
        ).Hash.ToLowerInvariant()
        if ($actual -ne $Matches[1]) {
            throw "Payload hash mismatch: $relative"
        }
    }
    $unhashed = @(
        Get-RelativeFiles $payload |
            Where-Object {
                $_ -ne "SHA256SUMS.txt" -and -not $hashedFiles.Contains($_)
            }
    )
    if ($unhashed.Count -ne 0) {
        throw "Files missing from SHA256SUMS.txt: $($unhashed -join ', ')"
    }

    if (-not $SkipIconProbe -and -not $IsWindows) {
        throw "The icon probe requires Windows; use -SkipIconProbe off-platform"
    }
    if (-not $SkipIconProbe -and -not ("PackageIconProbe" -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class PackageIconProbe {
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    public static extern uint ExtractIconEx(
        string file,
        int index,
        IntPtr[] large,
        IntPtr[] small,
        uint count
    );
}
'@
    }

    foreach ($name in @(
        "cockroach_overlay.exe",
        "cockroach_swarm_20.exe",
        "turtle_overlay.exe"
    )) {
        $executable = Join-Path $payload $name
        Assert-WindowsGuiPe $executable
        if (
            -not $SkipIconProbe -and
            [PackageIconProbe]::ExtractIconEx(
                $executable,
                -1,
                $null,
                $null,
                0
            ) -lt 1
        ) {
            throw "Executable contains no icon resource: $name"
        }
    }

    Write-Host "Verified Windows package: $Archive"
    Write-Host "SHA-256: $actualArchiveHash"
} finally {
    if ([System.IO.Directory]::Exists($extractRoot)) {
        [System.IO.Directory]::Delete($extractRoot, $true)
    }
}
