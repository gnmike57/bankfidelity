<#
.SYNOPSIS
    Step-by-step, self-diagnosing end-to-end smoke test for the
    dual-core-pdf-pipeline CLI.

.DESCRIPTION
    Runs the full pipeline (build -> test -> every CLI subcommand) one labelled
    step at a time. Each step:
      * prints a clear header,
      * tees its full output to out\max-test\<step>.log,
      * captures its exit code and duration,
      * is recorded as PASS / FAIL / SKIP.
    A final summary table is printed and written to summary.json, and the
    script exits non-zero if any step FAILed -- so it is safe in CI.

.PARAMETER Only
    Run only the named steps (step-by-step debugging). e.g. -Only render,extract

.PARAMETER Skip
    Skip the named steps. e.g. -Skip cargo-test

.PARAMETER StopOnError
    Abort at the first FAILed step instead of continuing.

.PARAMETER InputPdf
    Input PDF to exercise (default: .\test_doc.pdf).

.PARAMETER SkipBuild
    Do not auto-build; use whatever binary already exists.

.EXAMPLE
    .\run_max_test.ps1
.EXAMPLE
    .\run_max_test.ps1 -Only render,extract,balance
.EXAMPLE
    .\run_max_test.ps1 -StopOnError
#>
[CmdletBinding()]
param(
    [string[]]$Only = @(),
    [string[]]$Skip = @(),
    [switch]$StopOnError,
    [string]$InputPdf = ".\test_doc.pdf",
    [string]$OutDir = ".\out\max-test",
    [string]$Exe = ".\target\release\dual-core-pdf-pipeline.exe",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Continue"
$Results = [System.Collections.ArrayList]::new()

function Add-Result($Step, $Status, $ExitCode, $Seconds, $Log) {
    [void]$Results.Add([pscustomobject]@{
        Step = $Step; Status = $Status; Exit = $ExitCode; Seconds = $Seconds; Log = $Log
    })
}

function Write-Summary {
    Write-Host "`n================ STEP SUMMARY ================" -ForegroundColor Cyan
    ($Results | Format-Table Step, Status, Exit, Seconds -AutoSize | Out-String).TrimEnd() | Write-Host
    $pass = @($Results | Where-Object Status -eq 'PASS').Count
    $fail = @($Results | Where-Object Status -eq 'FAIL').Count
    $skip = @($Results | Where-Object Status -eq 'SKIP').Count
    Write-Host ("TOTAL {0}  PASS {1}  FAIL {2}  SKIP {3}" -f $Results.Count, $pass, $fail, $skip)
    $Results | ConvertTo-Json | Set-Content (Join-Path $OutDir 'summary.json')
}

function Invoke-Step {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$Log,
        [Parameter(Mandatory)][scriptblock]$Command,
        [bool]$When = $true
    )
    if ($Only.Count -and ($Only -notcontains $Name)) { return }
    if ($Skip -contains $Name) { Add-Result $Name 'SKIP' $null 0 'filtered (-Skip)'; Write-Host "`n=== [$Name] SKIP (filtered) ===" -ForegroundColor DarkGray; return }
    if (-not $When) { Add-Result $Name 'SKIP' $null 0 'precondition not met'; Write-Host "`n=== [$Name] SKIP (precondition not met) ===" -ForegroundColor DarkGray; return }

    Write-Host "`n=== [$Name] ===" -ForegroundColor Cyan
    $logPath = Join-Path $OutDir $Log
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $global:LASTEXITCODE = 0
    & $Command *>&1 | Tee-Object -FilePath $logPath
    $sw.Stop()
    $code = $global:LASTEXITCODE
    $secs = [math]::Round($sw.Elapsed.TotalSeconds, 1)
    $status = if ($code -eq 0) { 'PASS' } else { 'FAIL' }
    Add-Result $Name $status $code $secs $logPath
    $color = if ($status -eq 'PASS') { 'Green' } else { 'Red' }
    Write-Host ("--> {0}  (exit {1}, {2}s, log: {3})" -f $status, $code, $secs, $logPath) -ForegroundColor $color
    if ($status -eq 'FAIL' -and $StopOnError) {
        Write-Host "StopOnError set; aborting remaining steps." -ForegroundColor Red
        Write-Summary
        exit 1
    }
}

# --- Pre-flight -------------------------------------------------------------
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $OutDir "render") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $OutDir "verify") | Out-Null

Write-Host "== run_max_test : step-by-step pipeline smoke test ==" -ForegroundColor Cyan
Write-Host "  input  : $InputPdf"
Write-Host "  out    : $OutDir"
Write-Host "  binary : $Exe"

if (-not (Test-Path $InputPdf)) {
    Write-Host "WARNING: input '$InputPdf' not found; input-dependent steps will be skipped." -ForegroundColor Yellow
}

if ((-not (Test-Path $Exe)) -and (-not $SkipBuild)) {
    Write-Host "Binary missing; building release..." -ForegroundColor Yellow
    cargo build --release *>&1 | Tee-Object (Join-Path $OutDir "preflight_build.log") | Out-Null
}

$haveExe = Test-Path $Exe
$haveInput = Test-Path $InputPdf

# --- Build / test steps -----------------------------------------------------
Invoke-Step -Name 'cargo-check' -Log 'cargo_check.log' -Command { cargo check }
Invoke-Step -Name 'cargo-test'  -Log 'cargo_test_all_features.log' -Command { cargo test --all --all-features }
Invoke-Step -Name 'cargo-build' -Log 'cargo_build_release.log' -Command { cargo build --release }

# --- CLI smoke steps (need the binary) --------------------------------------
Invoke-Step -Name 'help'   -Log 'help.log'   -When $haveExe -Command { & $Exe --help }
Invoke-Step -Name 'doctor' -Log 'doctor.log' -When $haveExe -Command { & $Exe doctor }
Invoke-Step -Name 'verify-api-keys' -Log 'verify_api_keys.json' -When $haveExe -Command { & $Exe verify-api-keys --json }

Invoke-Step -Name 'selftest' -Log 'selftest.log' -When ($haveExe -and $haveInput) -Command { & $Exe selftest --input $InputPdf }
Invoke-Step -Name 'render'   -Log 'render.log'   -When ($haveExe -and $haveInput) -Command { & $Exe render -i $InputPdf -o (Join-Path $OutDir "render") -p 0 --dpi 150 }
Invoke-Step -Name 'extract'  -Log 'extract.log'  -When ($haveExe -and $haveInput) -Command { & $Exe extract -i $InputPdf -o (Join-Path $OutDir "transactions.json") }
Invoke-Step -Name 'balance'  -Log 'balance.log'  -When ($haveExe -and $haveInput) -Command { & $Exe balance -i $InputPdf -o (Join-Path $OutDir "balanced_proposed.pdf") }
Invoke-Step -Name 'auto-balance' -Log 'auto_balance.log' -When ($haveExe -and $haveInput) -Command { & $Exe auto-balance -i $InputPdf -o (Join-Path $OutDir "balanced.pdf") }

$balanced = Join-Path $OutDir "balanced.pdf"
Invoke-Step -Name 'verify' -Log 'verify.log' -When ($haveExe -and $haveInput -and (Test-Path $balanced)) -Command {
    & $Exe verify --original $InputPdf --edited $balanced --output-dir (Join-Path $OutDir "verify")
}

Invoke-Step -Name 'analyze-fonts' -Log 'analyze_fonts.log' -When ($haveExe -and $haveInput) -Command { & $Exe analyze-fonts -i $InputPdf }
Invoke-Step -Name 'adjust-dates'  -Log 'adjust_dates.log'  -When ($haveExe -and $haveInput) -Command { & $Exe adjust-dates -i $InputPdf -o (Join-Path $OutDir "dates_adjusted.pdf") --mode shift-forward-1-month }

$edited = ".\test_doc_edited.pdf"
Invoke-Step -Name 'transfer-transactions' -Log 'transfer_transactions.log' -When ($haveExe -and $haveInput -and (Test-Path $edited)) -Command {
    & $Exe transfer-transactions --source-pdf $InputPdf --target-pdf $edited -o (Join-Path $OutDir "transfer.pdf")
}

# Note: the GUI step is intentionally omitted here -- starting it would block
# this non-interactive smoke test. Launch it manually with: & $Exe gui

# --- Summary + exit code ----------------------------------------------------
Write-Summary
if (@($Results | Where-Object Status -eq 'FAIL').Count -gt 0) { exit 1 } else { exit 0 }
