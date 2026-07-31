param(
    [int] $SourceX = 37,
    [int] $SourceY = 547,
    [int] $TargetX = 50,
    [int] $TargetY = 340,
    [int] $HoldSeconds = 6
)

$ErrorActionPreference = "Stop"
$phasePath = Join-Path $env:PUBLIC "rust-bug-interaction-phase.txt"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class BugProbeInput {
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    public static extern void mouse_event(
        uint flags,
        uint dx,
        uint dy,
        uint data,
        UIntPtr extraInfo
    );
}
'@

$leftDown = 0x0002
$leftUp = 0x0004
$shell = New-Object -ComObject Shell.Application
$shell.MinimizeAll()
Start-Sleep -Seconds 2

$buttonIsDown = $false
try {
    if (-not [BugProbeInput]::SetCursorPos($SourceX, $SourceY)) {
        throw "SetCursorPos failed for the source point."
    }
    Start-Sleep -Milliseconds 300
    [BugProbeInput]::mouse_event($leftDown, 0, 0, 0, [UIntPtr]::Zero)
    $buttonIsDown = $true

    $steps = 24
    for ($step = 1; $step -le $steps; $step++) {
        $x = [Math]::Round(
            $SourceX + ($TargetX - $SourceX) * $step / $steps
        )
        $y = [Math]::Round(
            $SourceY + ($TargetY - $SourceY) * $step / $steps
        )
        if (-not [BugProbeInput]::SetCursorPos($x, $y)) {
            throw "SetCursorPos failed during drag step $step."
        }
        Start-Sleep -Milliseconds 25
    }

    "phase=holding x=$TargetX y=$TargetY" |
        Set-Content -LiteralPath $phasePath -Encoding ascii
    Start-Sleep -Seconds $HoldSeconds
    [BugProbeInput]::mouse_event($leftUp, 0, 0, 0, [UIntPtr]::Zero)
    $buttonIsDown = $false
    "phase=released x=$TargetX y=$TargetY" |
        Set-Content -LiteralPath $phasePath -Encoding ascii
}
catch {
    "phase=failed error=$($_.Exception.Message)" |
        Set-Content -LiteralPath $phasePath -Encoding ascii
    throw
}
finally {
    if ($buttonIsDown) {
        [BugProbeInput]::mouse_event(
            $leftUp,
            0,
            0,
            0,
            [UIntPtr]::Zero
        )
    }
}
