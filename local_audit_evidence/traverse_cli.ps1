$ErrorActionPreference = "Continue"

$commands = @(
    "gui", "mcp", "ufo", "chat", "serve", "text", "balance", 
    "extract", "extract-batch", "typst-reconstruct", "mcp-render-page", 
    "verify", "render", "font-complete", "export-history", "doctor", 
    "verify-api-keys", "docai-train", "fontcache-init", "analyze-fonts", 
    "auto-balance", "ai-fix-visual", "transfer-transactions", "adjust-dates", 
    "run-transfer-tests"
)

# Start job for each to allow timeout
foreach ($cmd in $commands) {
    Write-Host "Invoking: $cmd"
    $log_file = "local_audit_evidence/cli_output_$cmd.log"
    
    # Run the command. If it expects to hang (like serve), we can stop it after 5 seconds
    $job = Start-Job -ScriptBlock {
        param($cmd_name, $log)
        Set-Location "c:\bankfidelity\bankfidelity"
        # We run the command and hide secrets
        $output = cargo run -- $cmd_name 2>&1
        $output = $output -replace 'DUAL_CORE_PASSPHRASE=.*', 'DUAL_CORE_PASSPHRASE=[REDACTED]'
        $output = $output -replace 'GEMINI_API_KEY=.*', 'GEMINI_API_KEY=[REDACTED]'
        $output | Out-File $log
        return $LASTEXITCODE
    } -ArgumentList $cmd, $log_file

    # Wait up to 10 seconds
    Wait-Job -Job $job -Timeout 10 | Out-Null
    
    if ($job.State -eq 'Running') {
        Write-Host "$cmd timed out (likely a server or GUI). Stopping job."
        Stop-Job -Job $job
        "Job timed out and was stopped." | Out-File -Append $log_file
        $exitCode = "TIMEOUT"
    } else {
        $exitCode = Receive-Job -Job $job
    }
    Remove-Job -Job $job
    
    Write-Host "Exit Code for $cmd : $exitCode"
    "ExitCode: $exitCode" | Out-File -Append $log_file
}

Write-Host "CLI traversal complete."
