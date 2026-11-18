# Feuille de route

Objectif : rattraper les fonctionnalités de
[Yogurt](https://github.com/FrigadeHQ/yogurt), en Rust natif, en gardant
tout sur la machine de l'utilisateur.

## M0 — Squelette (fait)

- [x] Workspace Cargo : app (iced), audio, stt, llm, core
- [x] Capture microphone via cpal, rééchantillonnage 16 kHz
- [x] Transcription fenêtrée whisper.cpp (whisper-rs)
- [x] Stockage SQLite local (réunions, segments, résumés, notes)
- [x] Résumés via serveur LLM local compatible OpenAI
- [x] Export Markdown, CI 3 OS

## M1 — Enregistrement solide

- [ ] Chevauchement (1-2 s) entre fenêtres de transcription pour éviter les mots coupés
- [ ] Détection d'activité vocale (silero-vad ou heuristique d'énergie) : ne pas transcrire le silence
- [ ] Écriture audio en streaming sur disque (fichier WAV progressif) au lieu du buffer mémoire
- [ ] Renommer une réunion, éditer les notes depuis l'interface
- [ ] Gestion propre du thread de session en cas de panique (réalité : recharger l'UI)

## M2 — Prise de notes en direct

- [ ] Éditeur de notes pendant l'enregistrement
- [ ] « Obtenir un brouillon » : résumé intermédiaire pendant la réunion
- [ ] Titres de réunions générés automatiquement

## M3 — Audio système par OS

- [ ] Windows : WASAPI loopback (audio de l'appel)
- [ ] Linux : PulseAudio/PipeWire monitor source
- [ ] macOS : ScreenCaptureKit (autorisation système requise)
- [ ] Mixage micro + audio système, identification gauche/droite de l'appel

## M4 — Qui a dit quoi

- [ ] Diarisation (pyannote-like local, ou séparation de voix embarquée)
- [ ] Renommage des locuteurs depuis l'interface

## M5 — Détection d'appels

- [ ] Détecter les applications qui utilisent le micro (Zoom, Teams, Meet, navigateur)
- [ ] Proposer d'enregistrer quand un appel démarre ; arrêt automatique à la fin
- [ ] Intégration calendrier local (lire les agendas déjà connectés sur la machine)

## M6 — Recherche et organisation

- [ ] Recherche dans l'interface (SQLite FTS5, cité dans `Storage::search`)
- [ ] Dossiers, favoris, réponses avec citations de la réunion source

## M7 — Serveur MCP local

- [ ] Socket Unix 0600 (Linux/macOS) / pipe nommé à nom aléatoire (Windows)
- [ ] Outils en lecture seule : réunions, notes, transcriptions, audio, détails calendaires

## M8 — Diffusion

- [ ] AppImage + deb (Linux), MSI (Windows), dmg (macOS)
- [ ] Téléchargement et mise à jour des modèles depuis l'application
- [ ] Icône, thème, internationalisation (français, anglais d'abord)
