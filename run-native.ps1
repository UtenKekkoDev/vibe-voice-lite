$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$env:VIBE_TTS_RUNTIME_DIR = Join-Path $PSScriptRoot 'tts'
cargo run --locked --bin vibe-voice-lite
exit $LASTEXITCODE
