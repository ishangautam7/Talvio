# Prints which API keys the Talvio desktop app on this Windows PC holds, and the
# non-secret configuration around them (base URLs, models, voices), to choose
# the free-credit upstreams (README, "Free credit").
#
#   powershell -ExecutionPolicy Bypass -File server\scripts\inspect-desktop-keys.ps1
#
# It never prints a key, any part of one, or its length: a key is reported only
# as present or absent, plus which vendor's public key format it matches.

. "$PSScriptRoot\TalvioCredentials.ps1"

function Get-KeyShape([string]$Key) {
    if ($Key -like 'gsk_*') { return 'Groq-style' }
    if ($Key -like 'sk-or-*') { return 'OpenRouter-style' }
    if ($Key -like 'sk_*') { return 'ElevenLabs-style' }
    if ($Key -like 'sk-*') { return 'OpenAI-style' }
    if ($Key -match '^[0-9a-fA-F]{32}$' -or $Key.Length -ge 80) { return 'Azure-style' }
    return 'unrecognised format'
}

function Get-MapValue($Map, [string]$Name) {
    if ($null -eq $Map) { return $null }
    $prop = $Map.PSObject.Properties[$Name]
    if ($prop) { return $prop.Value }
    return $null
}

$settings = Get-TalvioSettings

Write-Output "== Keys in Windows Credential Manager"
$accounts = @(
    'post_process:azure_openai', 'post_process:openai', 'post_process:custom',
    'post_process:groq', 'post_process:openrouter', 'post_process:anthropic',
    'cloud_stt:elevenlabs', 'cloud_stt:openai', 'cloud_stt:groq', 'cloud_stt:deepgram',
    'assistant_tts:elevenlabs', 'assistant_tts:openai', 'assistant_tts:azure'
)
foreach ($account in $accounts) {
    $key = Get-TalvioSecret $account
    if ($null -ne $key) {
        Write-Output ("  {0,-28} present ({1})" -f $account, (Get-KeyShape $key))
    } else {
        Write-Output ("  {0,-28} -" -f $account)
    }
    $key = $null
}

Write-Output ""
Write-Output "== Chat providers (post_process_providers)"
foreach ($p in @($settings.post_process_providers)) {
    $id = $p.id
    $base = if ($p.base_url) { ConvertTo-TalvioBaseUrl $p.base_url } else { '' }
    $cleanup = Get-MapValue $settings.post_process_models $id
    $assistant = Get-MapValue $settings.assistant_models $id
    if ($base -or $cleanup -or $assistant) {
        Write-Output ("  {0,-14} base={1}  cleanupModel={2}  assistantModel={3}" -f $id, $base, $cleanup, $assistant)
    }
}
Write-Output ("  assistant provider: {0}   cleanup provider: {1}" -f $settings.assistant_provider_id, $settings.post_process_provider_id)

Write-Output ""
Write-Output "== Cloud speech-to-text"
Write-Output ("  provider: {0}" -f $settings.cloud_stt_provider_id)
foreach ($prop in @($settings.cloud_stt_models.PSObject.Properties)) {
    $base = Get-MapValue $settings.cloud_stt_base_urls $prop.Name
    Write-Output ("  {0,-14} model={1}  base={2}" -f $prop.Name, $prop.Value, $base)
}

Write-Output ""
Write-Output "== Spoken answers (TTS)"
Write-Output ("  engine: {0}" -f $settings.assistant_tts_engine)
foreach ($prop in @($settings.assistant_tts_models.PSObject.Properties)) {
    $voice = Get-MapValue $settings.assistant_tts_remote_voices $prop.Name
    $base = Get-MapValue $settings.assistant_tts_base_urls $prop.Name
    Write-Output ("  {0,-14} model={1}  voice={2}  base={3}" -f $prop.Name, $prop.Value, $voice, $base)
}
