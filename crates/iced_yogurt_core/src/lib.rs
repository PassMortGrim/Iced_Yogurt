//! Types métier et stockage 100 % local (SQLite) pour Iced Yogurt.
//!
//! Toutes les données — base, audio, réglages — vivent dans le dossier
//! renvoyé par [`data_dir`] et ne quittent jamais la machine.

use std::path::PathBuf;

use rusqlite::params;
use serde::{Deserialize, Serialize};

pub type SqlResult<T> = std::result::Result<T, rusqlite::Error>;

/// Horodatage « maintenant » en secondes UNIX.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Emplacement des données, par plateforme :
/// * Windows : `%APPDATA%\IcedYogurt`
/// * macOS : `~/Library/Application Support/IcedYogurt`
/// * Linux : `$XDG_DATA_HOME/IcedYogurt` (ou `~/.local/share/IcedYogurt`)
pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        return base.join("IcedYogurt");
    }
    #[cfg(target_os = "macos")]
    {
        let base = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        return base.join("Library/Application Support/IcedYogurt");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("."));
        return base.join("IcedYogurt");
    }
    #[allow(unreachable_code)]
    PathBuf::from(".")
}

/// Une réunion enregistrée.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub id: i64,
    pub title: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub notes: Option<String>,
    pub summary: Option<String>,
    pub audio_path: Option<String>,
}

/// Un fragment de transcription, en millisecondes depuis le début de l'enregistrement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub meeting_id: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: Option<String>,
    pub text: String,
}

fn meeting_from_row(row: &rusqlite::Row) -> rusqlite::Result<Meeting> {
    Ok(Meeting {
        id: row.get(0)?,
        title: row.get(1)?,
        started_at: row.get(2)?,
        ended_at: row.get(3)?,
        notes: row.get(4)?,
        summary: row.get(5)?,
        audio_path: row.get(6)?,
    })
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meetings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    notes TEXT,
    summary TEXT,
    audio_path TEXT
);
CREATE TABLE IF NOT EXISTS segments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    meeting_id INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    speaker TEXT,
    text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_segments_meeting ON segments(meeting_id, start_ms);
";

/// Base SQLite locale. Mono-fil de fait : l'interface l'utilise directement.
pub struct Storage {
    conn: rusqlite::Connection,
}

impl Storage {
    /// Ouvre (ou crée) la base dans [`data_dir`].
    pub fn open() -> SqlResult<Self> {
        let dir = data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let conn = rusqlite::Connection::open(dir.join("iced-yogurt.db"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn create_meeting(&self, title: &str, started_at: i64) -> SqlResult<i64> {
        self.conn.execute(
            "INSERT INTO meetings (title, started_at) VALUES (?1, ?2)",
            params![title, started_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_meeting(&self, id: i64, ended_at: i64, audio_path: Option<&str>) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE meetings SET ended_at = ?2, audio_path = ?3 WHERE id = ?1",
            params![id, ended_at, audio_path],
        )?;
        Ok(())
    }

    pub fn add_segment(&self, segment: &Segment) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO segments (meeting_id, start_ms, end_ms, speaker, text) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                segment.meeting_id,
                segment.start_ms,
                segment.end_ms,
                segment.speaker,
                segment.text
            ],
        )?;
        Ok(())
    }

    pub fn list_meetings(&self) -> SqlResult<Vec<Meeting>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, started_at, ended_at, notes, summary, audio_path
             FROM meetings ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map([], meeting_from_row)?;
        rows.collect()
    }

    pub fn get_meeting(&self, id: i64) -> SqlResult<Meeting> {
        self.conn.query_row(
            "SELECT id, title, started_at, ended_at, notes, summary, audio_path
             FROM meetings WHERE id = ?1",
            params![id],
            meeting_from_row,
        )
    }

    pub fn segments_for_meeting(&self, meeting_id: i64) -> SqlResult<Vec<Segment>> {
        let mut stmt = self.conn.prepare(
            "SELECT meeting_id, start_ms, end_ms, speaker, text
             FROM segments WHERE meeting_id = ?1 ORDER BY start_ms",
        )?;
        let rows = stmt.query_map(params![meeting_id], |row| {
            Ok(Segment {
                meeting_id: row.get(0)?,
                start_ms: row.get(1)?,
                end_ms: row.get(2)?,
                speaker: row.get(3)?,
                text: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Transcription d'une réunion assemblée en un seul texte.
    pub fn transcript_text(&self, meeting_id: i64) -> SqlResult<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT text FROM segments WHERE meeting_id = ?1 ORDER BY start_ms")?;
        let rows = stmt.query_map(params![meeting_id], |row| row.get::<_, String>(0))?;
        let mut parts = Vec::new();
        for row in rows {
            parts.push(row?);
        }
        Ok(parts.join("\n"))
    }

    pub fn set_summary(&self, id: i64, summary: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE meetings SET summary = ?2 WHERE id = ?1",
            params![id, summary],
        )?;
        Ok(())
    }

    pub fn set_notes(&self, id: i64, notes: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE meetings SET notes = ?2 WHERE id = ?1",
            params![id, notes],
        )?;
        Ok(())
    }

    /// Recherche simple : titre, notes et transcriptions.
    pub fn search(&self, query: &str) -> SqlResult<Vec<Meeting>> {
        let pattern = format!("%{}%", query.replace('%', ""));
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT m.id, m.title, m.started_at, m.ended_at, m.notes, m.summary, m.audio_path
             FROM meetings m LEFT JOIN segments s ON s.meeting_id = m.id
             WHERE m.title LIKE ?1 OR m.notes LIKE ?1 OR s.text LIKE ?1
             ORDER BY m.started_at DESC",
        )?;
        let rows = stmt.query_map(params![pattern], meeting_from_row)?;
        rows.collect()
    }
}

/// Réglages persistés en JSON dans le dossier de données.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Serveur local compatible OpenAI (Ollama, `llama-server`, LM Studio…).
    pub llm_base_url: String,
    /// Clé d'API : `ollama` convient pour un Ollama local.
    pub llm_api_key: String,
    /// Nom du modèle côté serveur (ex. `qwen3:8b`).
    pub llm_model: String,
    /// Chemin d'un modèle Whisper GGML (ex. `models/ggml-base.bin`).
    pub whisper_model_path: String,
    /// Langue attendue : `auto` ou un code ISO (`fr`, `en`…).
    pub transcription_language: String,
    /// Conserver le WAV de chaque réunion.
    pub save_audio: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            llm_base_url: "http://127.0.0.1:11434/v1".into(),
            llm_api_key: "ollama".into(),
            llm_model: "qwen3:8b".into(),
            whisper_model_path: "models/ggml-base.bin".into(),
            transcription_language: "auto".into(),
            save_audio: true,
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        data_dir().join("settings.json")
    }

    pub fn load() -> Self {
        match std::fs::read_to_string(Self::path()) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(Self::path(), json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn Settings_par_defaut() {
        let s = Settings::default();
        assert!(s.llm_base_url.starts_with("http://"));
        assert_eq!(s.transcription_language, "auto");
    }
}
