# ============================================================================
# BankFidelity Matrix Terminal Animation - ULTRA HACKER EDITION
# Fullscreen cinematic boot sequence with Matrix rain effect + Glitches
# Safe: Pure PowerShell, zero external dependencies, all errors caught
# ============================================================================

try {

# --- PHASE 0: Console Setup ---
try {
    Add-Type @"
    using System;
    using System.Runtime.InteropServices;
    public class ConsoleHelper {
        [DllImport("kernel32.dll")]
        public static extern IntPtr GetConsoleWindow();
        [DllImport("user32.dll")]
        public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    }
"@
    $hwnd = [ConsoleHelper]::GetConsoleWindow()
    [ConsoleHelper]::ShowWindow($hwnd, 3) | Out-Null
} catch {}

$Host.UI.RawUI.BackgroundColor = "Black"
$Host.UI.RawUI.ForegroundColor = "Green"
Clear-Host
[Console]::CursorVisible = $false
Start-Sleep -Milliseconds 100

$w = [Math]::Max(40, $Host.UI.RawUI.WindowSize.Width)
$h = [Math]::Max(10, $Host.UI.RawUI.WindowSize.Height)

function Set-Pos([int]$x, [int]$y) {
    if ($x -ge 0 -and $x -lt $w -and $y -ge 0 -and $y -lt $h) {
        try { $Host.UI.RawUI.CursorPosition = New-Object System.Management.Automation.Host.Coordinates $x, $y } catch {}
    }
}

function Write-Centered([string]$text, [int]$y, [string]$color = "Green") {
    $x = [Math]::Max(0, [Math]::Floor(($w - $text.Length) / 2))
    Set-Pos $x $y
    try { Write-Host -NoNewline -ForegroundColor $color $text } catch {}
}

$mchars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789@#%*+=-~:.,"
$hex = "0123456789ABCDEF"
$rng = New-Object System.Random

# ============================================================================
# PHASE 1: Neural Uplink Hex Dump (1s)
# ============================================================================
Clear-Host
for ($i = 0; $i -lt 40; $i++) {
    $line = ""
    for ($j = 0; $j -lt 16; $j++) {
        $line += "0x" + $hex[$rng.Next(16)] + $hex[$rng.Next(16)] + " "
    }
    $line += "  [SECURE UPLINK ESTABLISHED]"
    try { Write-Host -ForegroundColor DarkGreen $line } catch {}
    Start-Sleep -Milliseconds 10
}
Clear-Host
Start-Sleep -Milliseconds 200

# ============================================================================
# PHASE 2: Decrypting Master Cipher (1.5s)
# ============================================================================
$targetText = "BANKFIDELITY NEURAL ORCHESTRATOR"
$cx = [Math]::Max(0, [Math]::Floor(($w - $targetText.Length) / 2))
$cy = [Math]::Floor($h / 2)

$currentText = [char[]]("X" * $targetText.Length)
for ($cycle = 0; $cycle -lt 15; $cycle++) {
    Set-Pos $cx $cy
    for ($c = 0; $c -lt $targetText.Length; $c++) {
        if ($rng.Next(100) -gt 70) {
            $currentText[$c] = $targetText[$c]
        } elseif ($currentText[$c] -ne $targetText[$c]) {
            $currentText[$c] = $mchars[$rng.Next($mchars.Length)]
        }
    }
    try { Write-Host -NoNewline -ForegroundColor Cyan (-join $currentText) } catch {}
    Start-Sleep -Milliseconds 60
}
Set-Pos $cx $cy
try { Write-Host -NoNewline -ForegroundColor White $targetText } catch {}
Start-Sleep -Milliseconds 400

Write-Centered "[ BYPASSING FIREWALLS ]" ($cy + 2) "Red"
Start-Sleep -Milliseconds 600

# ============================================================================
# PHASE 3: Ultra Matrix Rain with Glitches (8 seconds)
# ============================================================================
Clear-Host

# Initialize dense columns (95% active)
$cols = New-Object object[] $w
for ($i = 0; $i -lt $w; $i++) {
    $cols[$i] = @{
        Y      = $rng.Next(-$h, 0)
        Speed  = $rng.Next(1, 4) # Faster!
        Len    = $rng.Next(8, [Math]::Max(10, $h))
        Active = ($rng.Next(0, 10) -ne 0) 
    }
}

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$rainDuration = 8000  # Increased to 8 seconds

while ($sw.ElapsedMilliseconds -lt $rainDuration) {
    for ($x = 0; $x -lt $w; $x++) {
        if (-not $cols[$x].Active) {
            if ($rng.Next(0, 50) -eq 0) { $cols[$x].Active = $true }
            continue
        }

        $c = $cols[$x]
        $headY = $c.Y

        # Draw head (bright white, occasional cyan/red glitch)
        if ($headY -ge 0 -and $headY -lt $h) {
            Set-Pos $x $headY
            $headColor = "White"
            if ($rng.Next(100) -gt 97) { $headColor = "Cyan" } # 3% glitch
            if ($rng.Next(100) -gt 98) { $headColor = "Red" }  # 2% glitch
            try { Write-Host -NoNewline -ForegroundColor $headColor $mchars[$rng.Next($mchars.Length)] } catch {}
        }

        # Draw bright trail 
        for ($t = 1; $t -le 2; $t++) {
            $ty = $headY - $t
            if ($ty -ge 0 -and $ty -lt $h) {
                Set-Pos $x $ty
                try { Write-Host -NoNewline -ForegroundColor Green $mchars[$rng.Next($mchars.Length)] } catch {}
            }
        }

        # Draw dim trail
        for ($t = 3; $t -lt $c.Len; $t++) {
            $ty = $headY - $t
            if ($ty -ge 0 -and $ty -lt $h) {
                Set-Pos $x $ty
                try { Write-Host -NoNewline -ForegroundColor DarkGreen $mchars[$rng.Next($mchars.Length)] } catch {}
            }
        }

        # Erase tail
        $tailY = $headY - $c.Len
        if ($tailY -ge 0 -and $tailY -lt $h) {
            Set-Pos $x $tailY
            try { Write-Host -NoNewline " " } catch {}
        }

        # Advance column
        $c.Y += $c.Speed
        if ($c.Y - $c.Len -gt $h) {
            $c.Y = $rng.Next(-10, -1)
            $c.Speed = $rng.Next(1, 4)
            $c.Len = $rng.Next(8, [Math]::Max(10, $h))
        }
    }
    Start-Sleep -Milliseconds 30 # Even faster loop
}

# ============================================================================
# PHASE 4: Rain Slowdown + Freeze (1.5s)
# ============================================================================
$slowSw = [System.Diagnostics.Stopwatch]::StartNew()
while ($slowSw.ElapsedMilliseconds -lt 1500) {
    $delay = 30 + [int]($slowSw.ElapsedMilliseconds / 10)
    for ($x = 0; $x -lt $w; $x += 2) { 
        if (-not $cols[$x].Active) { continue }
        $c = $cols[$x]
        $headY = $c.Y
        if ($headY -ge 0 -and $headY -lt $h) {
            Set-Pos $x $headY
            try { Write-Host -NoNewline -ForegroundColor DarkGreen $mchars[$rng.Next($mchars.Length)] } catch {}
        }
        $c.Y += 1
    }
    Start-Sleep -Milliseconds ([Math]::Min($delay, 150))
}

# ============================================================================
# PHASE 5: Logo Reveal (Faster & Crisper)
# ============================================================================
$logo = @(
    "    ____              __    _______     __     ___ __       "
    "   / __ )____ _____  / /__ / ____/ /____/ /____/ (_) /___  __"
    "  / __  / __ '/ __ \/ //_// /_  / //_/ / // __  / / / / / / /"
    " / /_/ / /_/ / / / / ,<  / __/ / /_/ / / // /_/ / / / / /_/ / "
    "/_____/\__,_/_/ /_/_/|_|/_/   /_____/_/_/ \__,_/_/_/_/\__, /  "
    "                                                     /____/   "
)
$subtitle = "T H E   M A T R I X   /   O R C H E S T R A T O R"

$logoStartY = [Math]::Floor(($h - $logo.Count - 4) / 2)
$maxLogoWidth = ($logo | ForEach-Object { $_.Length } | Measure-Object -Maximum).Maximum

$logoAreaTop = $logoStartY - 1
$logoAreaBottom = $logoStartY + $logo.Count + 4
$logoAreaLeft = [Math]::Max(0, [Math]::Floor(($w - $maxLogoWidth - 4) / 2))
$logoAreaRight = [Math]::Min($w - 1, $logoAreaLeft + $maxLogoWidth + 4)

# Glitch erase
for ($y = $logoAreaTop; $y -le $logoAreaBottom; $y++) {
    if ($y -ge 0 -and $y -lt $h) {
        for ($x = $logoAreaLeft; $x -le $logoAreaRight; $x++) {
            Set-Pos $x $y
            try { Write-Host -NoNewline " " } catch {}
        }
    }
}

$logoColors = @("DarkGreen", "DarkGreen", "Green", "Green", "White", "White")
for ($i = 0; $i -lt $logo.Count; $i++) {
    $line = $logo[$i]
    $lx = [Math]::Max(0, [Math]::Floor(($w - $line.Length) / 2))
    $ly = $logoStartY + $i
    $lColor = if ($i -lt $logoColors.Count) { $logoColors[$i] } else { "Green" }
    
    Set-Pos $lx $ly
    try { Write-Host -NoNewline -ForegroundColor $lColor $line } catch {}
    Start-Sleep -Milliseconds 40
}

$subY = $logoStartY + $logo.Count + 1
Write-Centered $subtitle $subY "Cyan"
Start-Sleep -Milliseconds 100
Write-Centered $subtitle $subY "White"

Start-Sleep -Milliseconds 600

# Progress bar
$barWidth = 50
$barX = [Math]::Max(0, [Math]::Floor(($w - $barWidth - 2) / 2))
$barY = $subY + 2
Set-Pos $barX $barY
try { Write-Host -NoNewline -ForegroundColor Cyan "[" } catch {}
Set-Pos ($barX + $barWidth + 1) $barY
try { Write-Host -NoNewline -ForegroundColor Cyan "]" } catch {}

for ($p = 0; $p -lt $barWidth; $p++) {
    Set-Pos ($barX + 1 + $p) $barY
    try { Write-Host -NoNewline -ForegroundColor White "=" } catch {}
    Start-Sleep -Milliseconds 10
}

Start-Sleep -Milliseconds 300

# Final flash
$Host.UI.RawUI.ForegroundColor = "White"
Start-Sleep -Milliseconds 50
$Host.UI.RawUI.ForegroundColor = "Green"
Start-Sleep -Milliseconds 50

# ============================================================================
# CLEANUP
# ============================================================================
[Console]::CursorVisible = $true
$Host.UI.RawUI.ForegroundColor = "Gray"
Clear-Host

} catch {
    try {
        [Console]::CursorVisible = $true
        Clear-Host
    } catch {}
}
