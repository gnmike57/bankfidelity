<#
.SYNOPSIS
  Run one allowlisted command as root on the GX10 through flak3dd's sudo, without the
  caller ever seeing the password.

.DESCRIPTION
  The sudo password is NOT in this file. It lives in $env:USERPROFILE\.gx10_sudo, encrypted with
  Windows DPAPI for the current Windows user (create it yourself - see SETUP below).
  This script decrypts it in-process, pipes it to `sudo -S` over SSH stdin, and never prints or
  logs it. The remote command must match the allowlist below and contain no shell metacharacters.
  Every call is appended to $env:USERPROFILE\.gx10_sudo.log (time, command, exit code).

  Login to flak3dd uses your SSH key (BatchMode): run once, typing the password yourself:
    ssh-copy-id -i ~/.ssh/id_ed25519.pub flak3dd@192.168.4.103

SETUP (you type the password; nobody else sees it):
  Read-Host -AsSecureString | ConvertFrom-SecureString | Set-Content $env:USERPROFILE\.gx10_sudo

USAGE
  .\gx10-sudo.ps1 -Check
  .\gx10-sudo.ps1 apt-get install -y libfreetype6-dev
  .\gx10-sudo.ps1 ls -la /home/flak3dd/gx10/models
  .\gx10-sudo.ps1 -DryRun docker ps        # validate only, no network, no secret

LIMITS (honest): anything running as your Windows user can decrypt the DPAPI file. The allowlist
and the Claude Code deny rules reduce that risk; they do not remove it. To revoke: delete
$env:USERPROFILE\.gx10_sudo and change the flak3dd password.
#>
[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Command,
    [switch]$Check,
    [switch]$DryRun
)
$ErrorActionPreference = 'Stop'

$TargetHost = if ($env:GX10_HOST) { $env:GX10_HOST } else { '192.168.4.103' }
$RemoteUser = 'flak3dd'
$SecretPath = Join-Path $env:USERPROFILE '.gx10_sudo'
$LogPath    = Join-Path $env:USERPROFILE '.gx10_sudo.log'

# Only these command shapes are ever sent. Widen deliberately, one line at a time.
$Allow = @(
    '^apt-get update$',
    '^apt-get install -y [a-z0-9][a-z0-9.+-]*( [a-z0-9][a-z0-9.+-]*)*$',
    '^docker (ps|ps -a|images|stats --no-stream)$',
    '^docker (logs|inspect|start|stop|restart) [A-Za-z0-9][A-Za-z0-9_.-]*$',
    '^ls( -[A-Za-z]+)? /home/flak3dd/gx10/models(/[A-Za-z0-9_.-]+)*/?$',
    '^du -sh /home/flak3dd/gx10/models(/[A-Za-z0-9_.-]+)*$',
    '^cp -r /home/flak3dd/gx10/models/[A-Za-z0-9][A-Za-z0-9_.-]* /home/nick/models/$',
    '^chown -R nick:nick /home/nick/models(/[A-Za-z0-9][A-Za-z0-9_.-]*)?$'
)

function Write-Log([string]$cmd, [string]$status) {
    $line = "{0}`t{1}`t{2}" -f (Get-Date -Format 's'), $status, $cmd
    Add-Content -LiteralPath $LogPath -Value $line -Encoding utf8
}

if ($Check) { $cmdLine = 'true' }
else {
    $cmdLine = (($Command -join ' ') -replace '\s+', ' ').Trim()
    if (-not $cmdLine) { throw 'no command given (use -Check to test the setup)' }
    if ($cmdLine -match '[;&|`$<>(){}\\"''*?\[\]!\r\n]' -or $cmdLine -match '\.\.') {
        Write-Log $cmdLine 'REJECTED-metachar'
        throw 'rejected: shell metacharacters or ".." are not allowed'
    }
    if (-not ($Allow | Where-Object { $cmdLine -match $_ })) {
        Write-Log $cmdLine 'REJECTED-allowlist'
        throw "rejected: command is not on the allowlist: $cmdLine"
    }
}

if ($DryRun) { Write-Output "ALLOWED (dry run, nothing sent): sudo $cmdLine"; return }

if (-not (Test-Path -LiteralPath $SecretPath)) {
    throw "no secret at $SecretPath - create it yourself (see SETUP in this script's header)"
}
$secure = (Get-Content -LiteralPath $SecretPath -Raw).Trim() | ConvertTo-SecureString
$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
try {
    $pw = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
    $out = $pw | & ssh -o BatchMode=yes -o ConnectTimeout=15 "$RemoteUser@$TargetHost" "sudo -S -p '' $cmdLine" 2>&1 |
        ForEach-Object { "$_" }
    $code = $LASTEXITCODE
}
finally {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
    $pw = $null
}
Write-Log $cmdLine ("exit=$code")
$out
if ($Check -and $code -eq 0) { Write-Output 'OK: key login and sudo both work' }
exit $code
