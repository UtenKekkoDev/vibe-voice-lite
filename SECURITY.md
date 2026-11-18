# Security Policy

## Supported Versions

The project is currently in alpha. Security fixes are handled on the latest `main` branch.

## Reporting A Vulnerability

Please do not open a public issue for vulnerabilities involving command execution, clipboard access, global hotkeys, model loading, or transcript disclosure.

Use GitHub private vulnerability reporting if it is enabled for the repository. If it is not enabled yet, contact the maintainer privately through the contact method listed on the GitHub profile.

When reporting, include:

- A clear description of the issue
- Steps to reproduce
- Affected commit or release
- Expected impact
- Any relevant logs with secrets removed

## Security Notes

- The app uses local microphone input.
- The app uses clipboard and keyboard simulation for delivery workflows.
- The app uses PowerShell only for the folder picker. Microphone callbacks do not write temporary state files.
- Settings include the AI service key encrypted with Windows DPAPI for the current user, bound to the configured endpoint. Clearing the key removes the saved value. Qwen-ASR keys remain session-only. Recordings and transcripts are kept out of settings.
- Paste checks the remembered external window and foreground focus; delivery errors preserve the transcript.
- Local SenseVoice and Qwen TTS inference do not upload content. Online Qwen-ASR sends audio to the configured service. AI editing and interviews send transcripts, glossary and conversation history to the selected endpoint. Remote AI services require HTTPS. Plain HTTP is accepted only for localhost or an IP loopback address.
- Model files are supplied by users and are not bundled with this repository.
