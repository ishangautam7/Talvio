# Copies the free-credit upstream settings and keys from this PC's Talvio
# install (Windows Credential Manager + settings; see TalvioCredentials.ps1,
# Get-FreeCreditEnv) into the server's root-only environment file, replacing
# any FREE_* lines already there, so running it again is safe.
#
#   powershell -ExecutionPolicy Bypass -File server\scripts\install-free-credit-env.ps1
#   powershell -ExecutionPolicy Bypass -File server\scripts\install-free-credit-env.ps1 -DryRun
#
# The keys never touch this PC's disk and are never printed: they are built in
# memory and piped over ssh's stdin into `sudo tee -a` on the server, which
# keeps the file mode 600, owned by root. Only variable names are shown.
# It does not restart the server; do that afterwards (README, "Free credit").

param(
    [string]$Server = "ubuntu@54.236.83.97",
    [string]$KeyFile = (Join-Path $HOME ".ssh\kiro.pem"),
    [string]$EnvFile = "/etc/talvio-server.env",
    # Show which variables would be written, and stop.
    [switch]$DryRun
)

. "$PSScriptRoot\TalvioCredentials.ps1"

if ($EnvFile -notmatch '^/[A-Za-z0-9._/-]+$') { throw "EnvFile must be a plain absolute path." }

$vars = Get-FreeCreditEnv
try {
    foreach ($name in $vars.Keys) {
        if ($vars[$name] -notmatch '^[\x21-\x7E]+$' -or $vars[$name] -match "['""\\`$``]") { throw "$name holds a character an env file line cannot carry." }
    }
    Write-Output "Variables for ${Server}:${EnvFile}:"
    foreach ($name in $vars.Keys) {
        $shown = if ($name -like '*_API_KEY') { '<from Credential Manager>' } else { $vars[$name] }
        Write-Output "  $name=$shown"
    }
    if ($DryRun) { return }
    if (-not (Test-Path $KeyFile)) { throw "No ssh key at $KeyFile" }

    # Drop every FREE_* line, then append the new ones from stdin. `tr` removes
    # the CR Windows PowerShell adds when it pipes to a native program, and sed
    # a byte-order mark should one be written anyway. No double quotes:
    # Windows PowerShell 5.1 mangles them in native arguments.
    $remote = "sudo touch $EnvFile && sudo chown root:root $EnvFile && sudo chmod 600 $EnvFile && " +
        "sudo sed -i '/^FREE_/d' $EnvFile && tr -d '\r' | sed '1s/^\xef\xbb\xbf//' | sudo tee -a $EnvFile > /dev/null && " +
        "printf 'FREE_ lines now in %s: ' $EnvFile && sudo grep -c '^FREE_' $EnvFile"

    $payload = ($vars.Keys | ForEach-Object { "$_=$($vars[$_])" }) -join "`n"
    $previousEncoding = $OutputEncoding
    # Every value is printable ASCII (checked above); ASCII also keeps a BOM out.
    $OutputEncoding = [System.Text.Encoding]::ASCII
    try {
        $payload | ssh -i $KeyFile -o BatchMode=yes $Server $remote
    } finally {
        $OutputEncoding = $previousEncoding
        $payload = $null
    }
    if ($LASTEXITCODE -ne 0) { throw "ssh failed (exit $LASTEXITCODE); the env file may be unchanged." }
} finally {
    $vars = $null
}
