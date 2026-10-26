# Runs scripts/free-live-check.ts with the free-credit upstreams chosen from
# this PC's Talvio keys (TalvioCredentials.ps1, Get-FreeCreditEnv): one tiny
# request each to ElevenLabs, Groq and Azure OpenAI, a few hundredths of a cent.
#
#   powershell -ExecutionPolicy Bypass -File server\scripts\live-check-free.ps1
#
# The keys go into this process's environment only, for the child `bun`, and
# are removed again afterwards; nothing is printed or written to disk.

. "$PSScriptRoot\TalvioCredentials.ps1"

$vars = Get-FreeCreditEnv
try {
    foreach ($name in $vars.Keys) { Set-Item -Path "Env:$name" -Value $vars[$name] }
    Push-Location (Join-Path $PSScriptRoot "..")
    try { bun scripts/free-live-check.ts } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "The live check failed." }
} finally {
    foreach ($name in $vars.Keys) { Remove-Item -Path "Env:$name" -ErrorAction SilentlyContinue }
    $vars = $null
}
