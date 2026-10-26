# Shared by the free-credit scripts: reads the desktop app's API keys out of
# Windows Credential Manager, where the app's keyring stores them as generic
# credentials named "<account>.com.talvio.desktop" (src-tauri/src/secret_store.rs).
#
# Dot-source it: `. "$PSScriptRoot\TalvioCredentials.ps1"`. Nothing here prints
# or writes a secret; callers decide what happens to the value.

$ErrorActionPreference = "Stop"

if (-not ("TalvioCred" -as [type])) {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class TalvioCred {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct CREDENTIAL {
        public int Flags; public int Type; public string TargetName; public string Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public int CredentialBlobSize; public IntPtr CredentialBlob; public int Persist;
        public int AttributeCount; public IntPtr Attributes; public string TargetAlias; public string UserName;
    }
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CredReadW(string target, int type, int flags, out IntPtr cred);
    [DllImport("advapi32.dll")]
    static extern void CredFree(IntPtr cred);
    // The raw blob, or null when there is no such generic credential.
    public static byte[] Read(string target) {
        IntPtr p;
        if (!CredReadW(target, 1, 0, out p)) return null;
        try {
            var c = (CREDENTIAL)Marshal.PtrToStructure(p, typeof(CREDENTIAL));
            var bytes = new byte[c.CredentialBlobSize];
            if (c.CredentialBlobSize > 0) Marshal.Copy(c.CredentialBlob, bytes, 0, c.CredentialBlobSize);
            return bytes;
        } finally { CredFree(p); }
    }
}
"@
}

$TalvioService = "com.talvio.desktop"

# The key stored for one keyring account (e.g. "post_process:azure_openai"), or
# $null. keyring writes UTF-16LE; a blob that does not decode as such is read as UTF-8.
function Get-TalvioSecret([string]$Account) {
    $bytes = [TalvioCred]::Read("$Account.$TalvioService")
    if ($null -eq $bytes -or $bytes.Length -eq 0) { return $null }
    $text = $null
    if ($bytes.Length % 2 -eq 0) {
        $candidate = [System.Text.Encoding]::Unicode.GetString($bytes)
        if ($candidate -match '^[\x20-\x7E]+$') { $text = $candidate }
    }
    if ($null -eq $text) { $text = [System.Text.Encoding]::UTF8.GetString($bytes) }
    $text = $text.Trim()
    if ($text.Length -eq 0) { return $null }
    return $text
}

# The desktop app's settings object (non-secret configuration).
function Get-TalvioSettings {
    $path = Join-Path $env:APPDATA "$TalvioService\settings_store.json"
    if (-not (Test-Path $path)) { throw "No Talvio settings at $path" }
    return (Get-Content $path -Raw | ConvertFrom-Json).settings
}

# Azure OpenAI-style hosts, normalized to their OpenAI-compatible /openai/v1
# base the same way llm_client.rs::effective_base_url does.
function ConvertTo-TalvioBaseUrl([string]$Raw) {
    $raw = $Raw.Trim().TrimEnd('/')
    if ($raw -match '^(https?://)?([^/]+)') {
        $hostName = $Matches[2].ToLower()
        foreach ($suffix in '.openai.azure.com', '.services.ai.azure.com', '.cognitiveservices.azure.com', '.openai.azure.us') {
            if ($hostName.EndsWith($suffix)) { return "https://$hostName/openai/v1" }
        }
    }
    return $raw
}

# The free-credit upstreams chosen from this PC's keys (server README, "Free
# credit"), as the FREE_* variables the server reads:
#   chat        Azure OpenAI, the assistant's deployment (gpt-5.6-luna)
#   transcribe  Groq whisper-large-v3-turbo, with the custom provider's Groq key
#   speech      ElevenLabs eleven_flash_v2_5, with the assistant voice's key and voice
# The values include API keys: keep them in memory, never print them or write
# them to a local file.
function Get-FreeCreditEnv {
    $settings = Get-TalvioSettings
    $azure = @($settings.post_process_providers | Where-Object { $_.id -eq 'azure_openai' })[0]
    $assistantModel = $settings.assistant_models.azure_openai
    $ttsVoice = $settings.assistant_tts_remote_voices.elevenlabs

    $chatKey = Get-TalvioSecret 'post_process:azure_openai'
    $sttKey = Get-TalvioSecret 'post_process:custom'
    $ttsKey = Get-TalvioSecret 'assistant_tts:elevenlabs'
    if (-not $chatKey) { throw "No Azure OpenAI key (post_process:azure_openai) in Credential Manager." }
    if (-not $azure.base_url) { throw "No Azure OpenAI endpoint in the Talvio settings." }
    if (-not $sttKey -or -not ($sttKey -like 'gsk_*')) { throw "The custom provider's key (post_process:custom) is not a Groq key." }
    if (-not $ttsKey) { throw "No ElevenLabs key (assistant_tts:elevenlabs) in Credential Manager." }

    $vars = [ordered]@{
        FREE_CHAT_BASE_URL = ConvertTo-TalvioBaseUrl $azure.base_url
        FREE_CHAT_API_KEY  = $chatKey
        FREE_CHAT_MODEL    = $(if ($assistantModel) { $assistantModel } else { 'gpt-5.6-luna' })
        FREE_STT_BASE_URL  = 'https://api.groq.com/openai/v1'
        FREE_STT_API_KEY   = $sttKey
        FREE_STT_MODEL     = 'whisper-large-v3-turbo'
        FREE_TTS_BASE_URL  = 'https://api.elevenlabs.io'
        FREE_TTS_API_KEY   = $ttsKey
        FREE_TTS_MODEL     = 'eleven_flash_v2_5'
    }
    if ($ttsVoice) { $vars['FREE_TTS_VOICE'] = $ttsVoice }
    return $vars
}
