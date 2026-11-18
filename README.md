# Iced Yogurt

> Private AI meeting notes that stay on your computer.
> A free, open-source alternative to Granola for Mac, Windows and Linux, rewritten in **Rust**.

**Iced Yogurt** est une réécriture en Rust de
[Yogurt](https://github.com/FrigadeHQ/yogurt) (projet **inspiré**, pas un fork :
aucun code partagé). Transcription et résumés 100 % locaux, aucun bot ne rejoint vos
réunions, aucun compte n'est nécessaire.

## État du projet

Le dépôt contient aujourd'hui un **squelette** : workspace Cargo à cinq crates, capture
microphone (cpal), boucle de transcription Whisper (whisper.cpp), résumés via un serveur
LLM local compatible OpenAI, stockage SQLite, export Markdown. La suite est cadrée dans
la [feuille de route](docs/roadmap.md).

## Architecture

| Crate               | Rôle                                                                            |
| ------------------- | ------------------------------------------------------------------------------- |
| `iced_yogurt_app`   | Interface [iced](https://iced.rs) et orchestration de la session d'enregistrement |
| `iced_yogurt_audio` | Capture microphone (cpal), rééchantillonnage 16 kHz, export WAV                  |
| `iced_yogurt_stt`   | Transcription locale : whisper.cpp via [whisper-rs](https://crates.io/crates/whisper-rs) |
| `iced_yogurt_llm`   | Client pour serveur local compatible OpenAI (Ollama, `llama-server`, LM Studio…) |
| `iced_yogurt_core`  | Types, base SQLite locale, réglages                                              |

## Prérequis

- Rust stable (`rustup`)
- `cmake` et une chaîne C/C++ (lien natif de `whisper.cpp`)
- Linux : `pkg-config`, `libasound2-dev`, `libudev-dev`

## Construire et lancer

```bash
cargo build --release
# binaire : target/release/iced-yogurt

mkdir -p models
curl -LO https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin

# Serveur LLM local (exemple avec Ollama ; llama-server ou LM Studio conviennent aussi)
ollama pull qwen3:8b
ollama serve
```

Les réglages (URL du serveur LLM, modèle, chemin du modèle Whisper, langue, sauvegarde
audio) sont dans `settings.json`, à côté de la base de données.

## Vos données restent chez vous

- Windows : `%APPDATA%\IcedYogurt`
- macOS : `~/Library/Application Support/IcedYogurt`
- Linux : `$XDG_DATA_HOME/IcedYogurt` (ou `~/.local/share/IcedYogurt`)

Rien n'est envoyé en ligne à part les exemples que vous poussez vous-même au serveur LLM
de votre choix.

## Feuille de route

Voir [docs/roadmap.md](docs/roadmap.md) : audio système par OS (WASAPI loopback,
monitor PulseAudio, ScreenCaptureKit), diarisation des locuteurs, détection d'appels,
recherche multi-réunions, serveur MCP local, packaging.

## Licence

MIT ; voir `LICENSE`. Projet inspiré par
[FrigadeHQ/yogurt](https://github.com/FrigadeHQ/yogurt), merci à eux pour la référence
clairement documentée.
