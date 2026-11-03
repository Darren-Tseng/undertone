use crate::settings::Settings;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntry {
    pub id: String,
    pub path: String,
    pub text: String,
    pub created: String,
    pub duration: f64,
    pub model: String,
    pub language: String,
}

pub struct NewTranscript<'a> {
    pub text: &'a str,
    pub duration: f64,
    pub model: &'a str,
    pub language: &'a str,
}

fn transcripts_root(settings: &Settings) -> PathBuf {
    PathBuf::from(&settings.workspace_dir).join("transcripts")
}

pub fn save(settings: &Settings, new: NewTranscript<'_>) -> Result<TranscriptEntry, String> {
    let text = new.text.trim();
    if text.is_empty() {
        return Err("沒有辨識到語音".to_string());
    }

    let now = chrono::Local::now();
    let folder = transcripts_root(settings).join(now.format("%Y-%m-%d").to_string());
    fs::create_dir_all(&folder).map_err(|e| format!("無法建立逐字稿資料夾：{e}"))?;

    let base = now.format("%H%M%S").to_string();
    let mut path = folder.join(format!("{base}.md"));
    let mut counter = 2;
    while path.exists() {
        path = folder.join(format!("{base}-{counter}.md"));
        counter += 1;
    }

    let created = now.to_rfc3339();
    let language = if new.language.is_empty() {
        settings.language.clone()
    } else {
        new.language.to_string()
    };

    let content = format!(
        "---\ncreated: {created}\nmodel: {}\nlanguage: {}\nduration: {:.2}\n---\n\n{}\n",
        new.model, language, new.duration, text
    );
    fs::write(&path, content).map_err(|e| format!("無法寫入逐字稿：{e}"))?;

    Ok(TranscriptEntry {
        id: path.to_string_lossy().to_string(),
        path: path.to_string_lossy().to_string(),
        text: text.to_string(),
        created,
        duration: new.duration,
        model: new.model.to_string(),
        language,
    })
}

pub fn list(settings: &Settings, query: &str, limit: usize) -> Vec<TranscriptEntry> {
    let root = transcripts_root(settings);
    let mut files: Vec<(PathBuf, SystemTime)> = Vec::new();

    let Ok(days) = fs::read_dir(&root) else {
        return Vec::new();
    };
    for day in days.flatten() {
        let Ok(kind) = day.file_type() else { continue };
        if !kind.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(day.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            files.push((path, modified));
        }
    }

    files.sort_by(|a, b| b.1.cmp(&a.1));
    files.truncate(2000);

    let needle = query.trim().to_lowercase();
    let mut entries: Vec<TranscriptEntry> = files
        .into_iter()
        .filter_map(|(path, _)| parse_entry(&path))
        .filter(|entry| needle.is_empty() || entry.text.to_lowercase().contains(&needle))
        .collect();

    entries.sort_by(|a, b| {
        let left = chrono::DateTime::parse_from_rfc3339(&a.created).ok();
        let right = chrono::DateTime::parse_from_rfc3339(&b.created).ok();
        right.cmp(&left)
    });
    entries.truncate(limit);
    entries
}

fn parse_entry(path: &Path) -> Option<TranscriptEntry> {
    let raw = fs::read_to_string(path).ok()?;
    let mut meta: Vec<(String, String)> = Vec::new();
    let mut lines = raw.lines();

    let text = if lines.next().map(str::trim) == Some("---") {
        for line in lines.by_ref() {
            let trimmed = line.trim();
            if trimmed == "---" {
                break;
            }
            if let Some((key, value)) = trimmed.split_once(':') {
                meta.push((key.trim().to_string(), value.trim().to_string()));
            }
        }
        lines.collect::<Vec<_>>().join("\n").trim().to_string()
    } else {
        raw.trim().to_string()
    };

    if text.is_empty() {
        return None;
    }

    let field = |name: &str| {
        meta.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };

    let fallback_created = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(|time| chrono::DateTime::<chrono::Local>::from(time).to_rfc3339())
        .unwrap_or_default();

    Some(TranscriptEntry {
        id: path.to_string_lossy().to_string(),
        path: path.to_string_lossy().to_string(),
        text,
        created: field("created").unwrap_or(fallback_created),
        duration: field("duration")
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0),
        model: field("model").unwrap_or_default(),
        language: field("language").unwrap_or_default(),
    })
}

pub fn delete(settings: &Settings, path: &str) -> Result<(), String> {
    let root = transcripts_root(settings);
    let root = root.canonicalize().unwrap_or(root);
    let target = PathBuf::from(path);
    let canonical = target
        .canonicalize()
        .map_err(|e| format!("找不到逐字稿：{e}"))?;

    if !canonical.starts_with(&root) {
        return Err("拒絕刪除工作區以外的檔案".to_string());
    }

    fs::remove_file(&canonical).map_err(|e| format!("無法刪除逐字稿：{e}"))
}

pub fn reveal(path: &str) -> Result<(), String> {
    let target = PathBuf::from(path);
    if !target.exists() {
        return Err("找不到逐字稿檔案".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", target.display()))
            .spawn()
            .map_err(|e| format!("無法開啟檔案總管：{e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open")
            .arg(target.parent().unwrap_or(&target))
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn open_workspace(settings: &Settings) -> Result<(), String> {
    let dir = PathBuf::from(&settings.workspace_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("無法建立工作區：{e}"))?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| format!("無法開啟資料夾：{e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings(dir: &Path) -> Settings {
        Settings {
            workspace_dir: dir.to_string_lossy().to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn saves_lists_searches_and_deletes() {
        let dir = std::env::temp_dir().join(format!("undertone-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let settings = test_settings(&dir);

        let entry = save(
            &settings,
            NewTranscript {
                text: "你好，世界。這是第二句話。",
                duration: 1.25,
                model: "small",
                language: "zh",
            },
        )
        .expect("save");

        assert!(Path::new(&entry.path).exists());
        assert!(entry.path.ends_with(".md"));

        let all = list(&settings, "", 10);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].text, "你好，世界。這是第二句話。");
        assert_eq!(all[0].language, "zh");
        assert!((all[0].duration - 1.25).abs() < 0.01);

        assert_eq!(list(&settings, "世界", 10).len(), 1);
        assert_eq!(list(&settings, "不存在", 10).len(), 0);

        delete(&settings, &entry.path).expect("delete");
        assert!(!Path::new(&entry.path).exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_empty_text() {
        let dir = std::env::temp_dir().join(format!("undertone-empty-{}", std::process::id()));
        let settings = test_settings(&dir);
        let result = save(
            &settings,
            NewTranscript {
                text: "   ",
                duration: 0.4,
                model: "small",
                language: "zh",
            },
        );
        assert!(result.is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_to_delete_outside_workspace() {
        let dir = std::env::temp_dir().join(format!("undertone-guard-{}", std::process::id()));
        let outside = std::env::temp_dir().join("undertone-outside.md");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&outside, "do not delete").unwrap();

        let settings = test_settings(&dir);
        assert!(delete(&settings, &outside.to_string_lossy()).is_err());
        assert!(outside.exists());

        let _ = fs::remove_file(&outside);
        let _ = fs::remove_dir_all(&dir);
    }
}
