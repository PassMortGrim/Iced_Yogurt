# Architecture

## Vue d'ensemble

Iced Yogurt est organisé en workspace Cargo de cinq crates, du plus bas niveau
au plus haut :

```
iced_yogurt_app        (binaire iced-yogurt, interface iced)
├── iced_yogurt_llm    (serveur local compatible OpenAI)
├── iced_yogurt_stt    (whisper.cpp via whisper-rs)
├── iced_yogurt_audio  (cpal, rééchantillonnage 16 kHz, WAV)
└── iced_yogurt_core   (types, SQLite, réglages)
```

## Flux d'un enregistrement

```
microphone (cpal)
   │  lots de f32 au taux natif
   ▼
Resampler ──► 16 kHz mono
   │
   ├──► buffer « transcription » ── fenêtres de 8 s ──► WhisperTranscriber
   │                                                       │
   │                                          segments (ms, texte)
   │                                                       ▼
   │                                            canal mpsc SessionEvent
   │                                                       ▼
   │                                            interface iced (update)
   │                                                     │
   │                                              SQLite + affichage
   │
   └──► buffer « audio complet » ── fin de session ──► reunion-<id>.wav
```

Points clés :

- Le `Recorder` (et donc le stream cpal) vit dans le fil de session :
  l'interface ne touche jamais au thread audio.
- La transcription est découpée en fenêtres de 8 secondes pour un retour
  régulier ; l'horloge (`processed_ms`) ajoute l'offset de chaque fenêtre.
- L'interface consomme les événements via une `Subscription` iced qui
  interroge le canal toutes les 50 ms.
- Les appels LLM sont asynchrones (`Task::perform`) et loin du chemin critique.

## Limites connues du squelette

- `iced_yogurt_audio` : un `Resampler` linéaire minimal ; la feuille de route
  prévoit un vrai resampler (rubato + filtre anti-repliement).
- `iced_yogurt_stt` : le fenêtrage fixe coupe parfois un mot à la frontière ;
  une marge de chevauchement est prévue en M1.
- L'audio système (ce que la réunion joue sur l'ordinateur) n'est pas encore
  capturé : c'est le cœur du M3, par OS (WASAPI loopback, monitor PulseAudio,
  ScreenCaptureKit).
