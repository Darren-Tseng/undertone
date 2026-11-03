use enigo::{Direction, Enigo, Key, Keyboard, Settings};

pub fn copy(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("無法存取剪貼簿：{e}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("無法寫入剪貼簿：{e}"))
}

/// Put the text on the clipboard and press Ctrl+V in whatever window has focus.
pub fn paste(text: &str) -> Result<(), String> {
    copy(text)?;

    // Give the clipboard a moment to settle before the synthetic keystroke.
    std::thread::sleep(std::time::Duration::from_millis(90));

    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("無法模擬鍵盤輸入：{e}"))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| e.to_string())?;
    let result = enigo.key(Key::Unicode('v'), Direction::Click);
    let _ = enigo.key(Key::Control, Direction::Release);
    result.map_err(|e| e.to_string())
}
