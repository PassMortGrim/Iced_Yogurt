//! Iced Yogurt : notes de réunion privées, 100 % locales.

mod recording;

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length, Subscription, Task};
use iced_yogurt_core::{data_dir, unix_now, Meeting, Segment, Settings, Storage};
use iced_yogurt_llm::{prompts, OpenAiClient};
use recording::{Session, SessionEvent};

fn main() -> iced::Result {
    iced::application("Iced Yogurt", update, view)
        .subscription(subscription)
        .run_with(State::init)
}

/// Une réunion ouverte pour lecture (résumé + transcription).
struct Selection {
    meeting: Meeting,
    segments: Vec<Segment>,
}

struct State {
    storage: Storage,
    settings: Settings,
    meetings: Vec<Meeting>,
    title_input: String,
    live_text: Vec<String>,
    recording: Option<Session>,
    status: String,
    selected: Option<Selection>,
}

impl State {
    fn init() -> (Self, Task<Message>) {
        let storage =
            Storage::open().expect("ouverture de la base de données locale impossible");
        let settings = Settings::load();
        let meetings = storage.list_meetings().unwrap_or_default();
        let status = format!(
            "LLM : {} · modèle : {} · micro : {}",
            settings.llm_base_url,
            settings.llm_model,
            iced_yogurt_audio::default_input_name().unwrap_or_else(|| "aucun".into()),
        );
        (
            State {
                storage,
                settings,
                meetings,
                title_input: String::new(),
                live_text: Vec::new(),
                recording: None,
                status,
                selected: None,
            },
            Task::none(),
        )
    }

    fn load_selection(&self, id: i64) -> anyhow::Result<Selection> {
        Ok(Selection {
            meeting: self.storage.get_meeting(id)?,
            segments: self.storage.segments_for_meeting(id)?,
        })
    }
}

#[derive(Debug, Clone)]
enum Message {
    StartRecording,
    StopRecording,
    SessionEvent(SessionEvent),
    TitleChanged(String),
    MeetingSelected(i64),
    Summarize(i64),
    SummaryDone(i64, Result<String, String>),
    Export(i64),
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::StartRecording => start_recording(state),
        Message::StopRecording => {
            if let Some(session) = &state.recording {
                session.request_stop();
                state.status = "Arrêt demandé : dernières secondes en transcription…".into();
            }
            Task::none()
        }
        Message::SessionEvent(event) => on_session_event(state, event),
        Message::TitleChanged(value) => {
            state.title_input = value;
            Task::none()
        }
        Message::MeetingSelected(id) => match state.load_selection(id) {
            Ok(selection) => {
                state.selected = Some(selection);
                Task::none()
            }
            Err(err) => {
                state.status = format!("Lecture impossible : {err}");
                Task::none()
            }
        },
        Message::Summarize(id) => summarize(state, id),
        Message::SummaryDone(id, Ok(summary)) => {
            let _ = state.storage.set_summary(id, &summary);
            state.meetings = state.storage.list_meetings().unwrap_or_default();
            let selected_is_current = state.selected.as_ref().is_some_and(|s| s.meeting.id == id);
            if selected_is_current {
                if let Ok(selection) = state.load_selection(id) {
                    state.selected = Some(selection);
                }
            }
            state.status = "Résumé enregistré.".into();
            Task::none()
        }
        Message::SummaryDone(_, Err(err)) => {
            state.status = format!("Résumé impossible : {err}");
            Task::none()
        }
        Message::Export(id) => export_markdown(state, id),
    }
}

fn start_recording(state: &mut State) -> Task<Message> {
    if state.recording.is_some() {
        return Task::none();
    }
    let Ok(recorder) = iced_yogurt_audio::Recorder::start() else {
        state.status = "Impossible d'ouvrir le microphone.".into();
        return Task::none();
    };

    let title = {
        let trimmed = state.title_input.trim();
        if trimmed.is_empty() {
            "Réunion sans titre"
        } else {
            trimmed
        }
    };
    let meeting_id = match state.storage.create_meeting(title, unix_now()) {
        Ok(id) => id,
        Err(err) => {
            state.status = format!("Base de données : {err}");
            return Task::none();
        }
    };

    let audio_dir = data_dir().join("audio");
    let _ = std::fs::create_dir_all(&audio_dir);
    let model = state.settings.whisper_model_path.trim().to_string();
    let kyutai_config = if state.settings.use_kyutai_stt {
        Some(iced_yogurt_stt::MoshiServerConfig {
            base_url: state.settings.kyutai_stt_server_url.clone(),
            model: state.settings.kyutai_stt_model.clone(),
            language: Some(state.settings.transcription_language.clone()),
        })
    } else {
        None
    };
    let session = match recording::start(
        recorder,
        meeting_id,
        if model.is_empty() { None } else { Some(model) },
        &state.settings.transcription_language,
        audio_dir,
        state.settings.save_audio,
        state.settings.use_kyutai_stt,
        kyutai_config,
        state.settings.kyutai_window_seconds,
        state.settings.kyutai_overlap_seconds,
    ) {
        Ok(session) => session,
        Err(err) => {
            state.status = format!("Session : {err}");
            return Task::none();
        }
    };

    state.live_text.clear();
    state.recording = Some(session);
    state.status = "Enregistrement en cours…".into();
    Task::none()
}

fn on_session_event(state: &mut State, event: SessionEvent) -> Task<Message> {
    match event {
        SessionEvent::Segments(segments) => {
            let Some(session) = &state.recording else {
                return Task::none();
            };
            let meeting_id = session.meeting_id;
            for seg in segments {
                let segment = Segment {
                    meeting_id,
                    start_ms: seg.start_ms as i64,
                    end_ms: seg.end_ms as i64,
                    speaker: None,
                    text: seg.text,
                };
                if state.storage.add_segment(&segment).is_ok() {
                    state.live_text.push(segment.text);
                }
            }
            Task::none()
        }
        SessionEvent::AudioSaved(path) => {
            let Some(session) = &state.recording else {
                return Task::none();
            };
            let _ = state
                .storage
                .finish_meeting(session.meeting_id, unix_now(), path.as_deref());
            Task::none()
        }
        SessionEvent::Finished => {
            state.recording = None;
            state.meetings = state.storage.list_meetings().unwrap_or_default();
            state.status = "Enregistrement terminé, réunion archivée.".into();
            Task::none()
        }
        SessionEvent::Warned(message) => {
            state.status = message;
            Task::none()
        }
    }
}

fn summarize(state: &mut State, id: i64) -> Task<Message> {
    let client = OpenAiClient::new(
        &state.settings.llm_base_url,
        &state.settings.llm_api_key,
        &state.settings.llm_model,
    );
    let Ok(client) = client else {
        state.status = "URL de serveur LLM invalide, vérifiez settings.json.".into();
        return Task::none();
    };
    let transcript = match state.storage.transcript_text(id) {
        Ok(text) if !text.is_empty() => text,
        Ok(_) => {
            state.status = "Pas encore assez de texte transcrit pour résumer.".into();
            return Task::none();
        }
        Err(err) => {
            state.status = format!("Base de données : {err}");
            return Task::none();
        }
    };
    let notes = state.storage.get_meeting(id).ok().and_then(|m| m.notes);
    state.status = format!("Résumé en cours avec {}…", client.model());
    let system = prompts::summary_system().to_string();
    let user = prompts::summary_user(&transcript, notes.as_deref());
    Task::perform(
        async move { client.clone().chat(&system, &user).await },
        move |result| Message::SummaryDone(id, result.map_err(|e| e.to_string())),
    )
}

fn export_markdown(state: &mut State, id: i64) -> Task<Message> {
    let meeting = match state.storage.get_meeting(id) {
        Ok(m) => m,
        Err(err) => {
            state.status = format!("Base de données : {err}");
            return Task::none();
        }
    };
    let segments = state.storage.segments_for_meeting(id).unwrap_or_default();

    let mut out = format!("# {}\n\n", meeting.title);
    if let Some(summary) = &meeting.summary {
        out.push_str("## Résumé\n\n");
        out.push_str(summary);
        out.push_str("\n\n");
    }
    out.push_str("## Transcription\n\n");
    for seg in &segments {
        let minutes = seg.start_ms / 60_000;
        let seconds = (seg.start_ms % 60_000) / 1000;
        out.push_str(&format!("[{minutes:02}:{seconds:02}] {}\n", seg.text));
    }

    let export_dir = data_dir().join("export");
    let path = export_dir.join(format!("reunion-{id}.md"));
    match std::fs::create_dir_all(&export_dir).and_then(|()| std::fs::write(&path, out)) {
        Ok(()) => state.status = format!("Exporté : {}", path.display()),
        Err(err) => state.status = format!("Export impossible : {err}"),
    }
    Task::none()
}

fn subscription(state: &State) -> Subscription<Message> {
    let Some(session) = &state.recording else {
        return Subscription::none();
    };
    let events = session.events.clone();
    Subscription::run_with_id(
        ("iced-yogurt-session", session.meeting_id),
        iced::stream::channel(100, move |mut sender| async move {
            loop {
                let event = {
                    let Ok(events) = events.lock() else { break };
                    events.try_recv()
                };
                match event {
                    Ok(received) => {
                        if sender.try_send(Message::SessionEvent(received)).is_err() {
                            break;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        }),
    )
}

fn view(state: &State) -> Element<'_, Message> {
    let mut root = column![].spacing(12).padding(20).max_width(720);

    if state.recording.is_some() {
        root = root.push(
            text("● Enregistrement en cours")
                .size(24)
                .color(iced::Color::from_rgb(0.85, 0.2, 0.2)),
        );
        root = root.push(button("Arrêter l'enregistrement").on_press(Message::StopRecording));

        let mut live = column![].spacing(4);
        for line in &state.live_text {
            live = live.push(text(line).size(14));
        }
        root = root.push(scrollable(live).height(Length::FillPortion(2)));
    } else {
        root = root.push(text("Iced Yogurt").size(24));
        root = root.push(
            row![
                text_input("Titre de la nouvelle réunion", &state.title_input)
                    .on_input(Message::TitleChanged),
                button("Enregistrer").on_press(Message::StartRecording),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }

    root = root.push(text(&state.status).size(12));

    root = root.push(text("Réunions récentes").size(16));
    let mut list = column![].spacing(6);
    for meeting in &state.meetings {
        list = list.push(
            row![
                button(text(meeting.title.clone()).size(14))
                    .on_press(Message::MeetingSelected(meeting.id)),
                button("Résumé").on_press(Message::Summarize(meeting.id)),
                button("Export").on_press(Message::Export(meeting.id)),
            ]
            .spacing(6),
        );
    }
    root = root.push(list);

    if let Some(selected) = &state.selected {
        root = root.push(text(selected.meeting.title.clone()).size(18));
        if let Some(summary) = &selected.meeting.summary {
            root = root.push(text("Résumé").size(14));
            root = root.push(text(summary.clone()).size(13));
        }
        let mut transcript = column![].spacing(4);
        for seg in &selected.segments {
            let minutes = seg.start_ms / 60_000;
            let seconds = (seg.start_ms % 60_000) / 1000;
            transcript = transcript.push(text(format!("[{minutes:02}:{seconds:02}] {}", seg.text)));
        }
        root = root.push(scrollable(transcript).height(Length::FillPortion(3)));
    }

    container(scrollable(root).height(Length::Fill))
        .height(Length::Fill)
        .into()
}
