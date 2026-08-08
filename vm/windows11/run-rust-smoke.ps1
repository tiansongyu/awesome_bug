$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$mediaRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$installRoot = "C:\RustBugTest"
$resultPath = Join-Path $env:PUBLIC "rust-bug-vm-result.txt"
$singleTrace = Join-Path $env:PUBLIC "rust-bug-single.tsv"
$swarmTrace = Join-Path $env:PUBLIC "rust-bug-swarm.tsv"
$turtleTrace = Join-Path $env:PUBLIC "rust-bug-turtle.tsv"

Get-Process cockroach_overlay, cockroach_swarm_20, turtle_overlay `
    -ErrorAction SilentlyContinue |
    Stop-Process -Force

if (Test-Path -LiteralPath $installRoot) {
    Remove-Item -LiteralPath $installRoot -Recurse -Force
}
New-Item -ItemType Directory -Path $installRoot | Out-Null
Copy-Item -LiteralPath (Join-Path $mediaRoot "cockroach_overlay.exe") `
    -Destination $installRoot
Copy-Item -LiteralPath (Join-Path $mediaRoot "cockroach_swarm_20.exe") `
    -Destination $installRoot
Copy-Item -LiteralPath (Join-Path $mediaRoot "turtle_overlay.exe") `
    -Destination $installRoot
Copy-Item -LiteralPath (Join-Path $mediaRoot "SDL2.dll") `
    -Destination $installRoot
Copy-Item -LiteralPath (Join-Path $mediaRoot "bugs") `
    -Destination $installRoot -Recurse

Remove-Item -LiteralPath $singleTrace, $swarmTrace, $turtleTrace, $resultPath `
    -Force -ErrorAction SilentlyContinue

function Invoke-BoundedCase {
    param(
        [Parameter(Mandatory = $true)][string] $Executable,
        [Parameter(Mandatory = $true)][string[]] $Arguments,
        [Parameter(Mandatory = $true)][int] $TimeoutMilliseconds
    )

    $process = Start-Process -FilePath $Executable `
        -ArgumentList $Arguments `
        -WorkingDirectory $env:TEMP `
        -PassThru
    if (-not $process.WaitForExit($TimeoutMilliseconds)) {
        $process.Kill()
        throw "Timed out: $Executable $($Arguments -join ' ')"
    }
    if ($process.ExitCode -ne 0) {
        throw "Exit $($process.ExitCode): $Executable $($Arguments -join ' ')"
    }
}

function Get-MaximumStep {
    param([Parameter(Mandatory = $true)][object[]] $Rows)

    $maximum = 0.0
    foreach ($row in $Rows) {
        $dx = [double]::Parse(
            $row.displacement_x,
            [Globalization.CultureInfo]::InvariantCulture
        )
        $dy = [double]::Parse(
            $row.displacement_y,
            [Globalization.CultureInfo]::InvariantCulture
        )
        $distance = [Math]::Sqrt($dx * $dx + $dy * $dy)
        if ($distance -gt $maximum) {
            $maximum = $distance
        }
    }
    return $maximum
}

$lines = [System.Collections.Generic.List[string]]::new()
$lines.Add("Rust + Lua Windows 11 VM smoke")
$lines.Add("timestamp=$([DateTimeOffset]::Now.ToString('o'))")
$lines.Add("windows=$([Environment]::OSVersion.VersionString)")
$resultPassed = $false

try {
    Invoke-BoundedCase `
        (Join-Path $installRoot "cockroach_overlay.exe") `
        @("--frames", "360", "--seed", "424242", "--trace", $singleTrace) `
        30000
    $single = @(Import-Csv -LiteralPath $singleTrace -Delimiter "`t")
    if ($single.Count -ne 360) {
        throw "single trace row count is $($single.Count), expected 360"
    }
    if (@($single.instance | Sort-Object -Unique).Count -ne 1) {
        throw "single trace did not contain exactly one instance"
    }
    if (@($single | Where-Object quarantined -eq "1").Count -ne 0) {
        throw "single controller was quarantined"
    }
    $singleMaximumStep = Get-MaximumStep $single
    if ($singleMaximumStep -gt 40.0) {
        throw "single trace contains a teleport-sized $singleMaximumStep px step"
    }
    $lines.Add("single=PASS rows=$($single.Count) states=$(
        @($single.state | Sort-Object -Unique) -join ',') max_step=$(
        $singleMaximumStep.ToString(
            '0.000',
            [Globalization.CultureInfo]::InvariantCulture
        ))")

    Invoke-BoundedCase `
        (Join-Path $installRoot "cockroach_swarm_20.exe") `
        @("--frames", "120", "--seed", "424242", "--trace", $swarmTrace) `
        30000
    $swarm = @(Import-Csv -LiteralPath $swarmTrace -Delimiter "`t")
    if ($swarm.Count -ne 2400) {
        throw "swarm trace row count is $($swarm.Count), expected 2400"
    }
    if (@($swarm.instance | Sort-Object -Unique).Count -ne 20) {
        throw "swarm trace did not contain exactly 20 instances"
    }
    if (@($swarm | Where-Object quarantined -eq "1").Count -ne 0) {
        throw "a swarm controller was quarantined"
    }
    $swarmMaximumStep = Get-MaximumStep $swarm
    if ($swarmMaximumStep -gt 40.0) {
        throw "swarm trace contains a teleport-sized $swarmMaximumStep px step"
    }
    $lines.Add("swarm=PASS rows=$($swarm.Count) instances=20 max_step=$(
        $swarmMaximumStep.ToString(
            '0.000',
            [Globalization.CultureInfo]::InvariantCulture
        ))")

    Invoke-BoundedCase `
        (Join-Path $installRoot "turtle_overlay.exe") `
        @("--frames", "600", "--seed", "424242", "--trace", $turtleTrace) `
        30000
    $turtle = @(Import-Csv -LiteralPath $turtleTrace -Delimiter "`t")
    if ($turtle.Count -ne 600) {
        throw "turtle trace row count is $($turtle.Count), expected 600"
    }
    if (@($turtle.instance | Sort-Object -Unique).Count -ne 1) {
        throw "turtle trace did not contain exactly one instance"
    }
    if (@($turtle | Where-Object quarantined -eq "1").Count -ne 0) {
        throw "turtle controller was quarantined"
    }
    $turtleMaximumStep = Get-MaximumStep $turtle
    if ($turtleMaximumStep -gt 20.0) {
        throw "turtle trace contains a teleport-sized $turtleMaximumStep px step"
    }
    $lines.Add("turtle=PASS rows=$($turtle.Count) states=$(
        @($turtle.state | Sort-Object -Unique) -join ',') max_step=$(
        $turtleMaximumStep.ToString(
            '0.000',
            [Globalization.CultureInfo]::InvariantCulture
        ))")
    $lines.Add("result=PASS")
    $resultPassed = $true
} catch {
    $lines.Add("result=FAIL")
    $lines.Add("error=$($_.Exception.Message)")
} finally {
    foreach ($name in "cockroach_overlay", "cockroach_swarm_20", "turtle_overlay") {
        Get-Process $name -ErrorAction SilentlyContinue | Stop-Process -Force
    }
    $lines.Add("overlay_sha256=$(
        (Get-FileHash -LiteralPath (
            Join-Path $installRoot 'cockroach_overlay.exe'
        ) -Algorithm SHA256).Hash.ToLowerInvariant())")
    $lines.Add("swarm_sha256=$(
        (Get-FileHash -LiteralPath (
            Join-Path $installRoot 'cockroach_swarm_20.exe'
        ) -Algorithm SHA256).Hash.ToLowerInvariant())")
    $lines.Add("turtle_sha256=$(
        (Get-FileHash -LiteralPath (
            Join-Path $installRoot 'turtle_overlay.exe'
        ) -Algorithm SHA256).Hash.ToLowerInvariant())")
    $lines | Set-Content -LiteralPath $resultPath -Encoding utf8
    Start-Process notepad.exe -ArgumentList $resultPath
}

if (-not $resultPassed) {
    exit 1
}
