$ErrorActionPreference = "Stop"

$mediaRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$tracePath = Join-Path $env:PUBLIC "rust-bug-bait.tsv"
$resultPath = Join-Path $env:PUBLIC "rust-bug-bait-result.txt"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class BugProbeKeys {
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    public static extern void keybd_event(
        byte virtualKey,
        byte scanCode,
        uint flags,
        UIntPtr extraInfo
    );
}
'@

Get-Process cockroach_overlay, cockroach_swarm_20 -ErrorAction SilentlyContinue |
    Stop-Process -Force
Remove-Item -LiteralPath $tracePath, $resultPath `
    -Force -ErrorAction SilentlyContinue

$arguments = @(
    "--frames", "1800",
    "--seed", "20260731",
    "--trace", $tracePath
)
$process = Start-Process `
    -FilePath (Join-Path $mediaRoot "cockroach_overlay.exe") `
    -ArgumentList $arguments `
    -WorkingDirectory $env:TEMP `
    -PassThru

$shell = New-Object -ComObject Shell.Application
$shell.MinimizeAll()
Start-Sleep -Seconds 2
[BugProbeKeys]::SetCursorPos(640, 400) | Out-Null

$keyUp = 0x0002
foreach ($key in 0x11, 0x12, 0x46) {
    [BugProbeKeys]::keybd_event($key, 0, 0, [UIntPtr]::Zero)
}
Start-Sleep -Milliseconds 200
foreach ($key in 0x46, 0x12, 0x11) {
    [BugProbeKeys]::keybd_event($key, 0, $keyUp, [UIntPtr]::Zero)
}

$passed = $false
$lines = [System.Collections.Generic.List[string]]::new()
try {
    if (-not $process.WaitForExit(40000)) {
        $process.Kill()
        throw "The bounded bait trace timed out."
    }
    if ($process.ExitCode -ne 0) {
        throw "The overlay exited with code $($process.ExitCode)."
    }

    $rows = @(Import-Csv -LiteralPath $tracePath -Delimiter "`t")
    if ($rows.Count -ne 1800) {
        throw "Trace row count is $($rows.Count), expected 1800."
    }
    if (@($rows | Where-Object quarantined -eq "1").Count -ne 0) {
        throw "The Lua controller was quarantined."
    }
    $states = @($rows.state | Sort-Object -Unique)
    if ($states -notcontains "seek-food" -or $states -notcontains "feeding") {
        throw "Food states were not both observed: $($states -join ',')."
    }

    $lines.Add("result=PASS")
    $lines.Add("rows=$($rows.Count)")
    $lines.Add("states=$($states -join ',')")
    $firstSeek = @($rows | Where-Object state -eq "seek-food")[0]
    $firstFeeding = @($rows | Where-Object state -eq "feeding")[0]
    $lines.Add("first_seek_frame=$($firstSeek.frame)")
    $lines.Add("first_feeding_frame=$($firstFeeding.frame)")
    $consumeCount = @($rows | Where-Object consume_bait -eq "1").Count
    $lines.Add("consume_bait=$consumeCount")
    $passed = $true
} catch {
    $lines.Add("result=FAIL")
    $lines.Add("error=$($_.Exception.Message)")
} finally {
    $lines | Set-Content -LiteralPath $resultPath -Encoding utf8
    Start-Process notepad.exe -ArgumentList $resultPath
}

if (-not $passed) {
    exit 1
}
