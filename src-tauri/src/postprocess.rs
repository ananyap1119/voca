use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const DANDA: char = '\u{0964}';

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    content: String,
}

pub async fn polish_transcript(input: &str, config: &Config) -> Result<String, String> {
    let local = light_polish(input);
    match config.polish_mode.as_deref().unwrap_or("off") {
        "off" => Ok(input.trim().to_string()),
        "full" => full_polish(&local, config).await,
        _ => Ok(local),
    }
}

pub fn light_polish(input: &str) -> String {
    let disfluent = remove_disfluencies(input);
    let commanded = apply_voice_commands(&disfluent);
    let collapsed = collapse_spaces_preserving_breaks(&commanded);
    let no_space_before_punctuation = remove_space_before_punctuation(&collapsed);
    let spaced_after_punctuation = add_space_after_punctuation(&no_space_before_punctuation);
    finish_sentence(spaced_after_punctuation.trim())
}

async fn full_polish(input: &str, config: &Config) -> Result<String, String> {
    let endpoint = match config.polish_endpoint.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => value,
        _ => return Ok(input.to_string()),
    };
    let model = match config.polish_model.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => value,
        _ => return Ok(input.to_string()),
    };

    let system_prompt = "You clean dictation transcripts for typing. Preserve the speaker's meaning, language, script, names, and code-mixing. Remove filler words, repeated hesitation phrases, and spoken planning clutter. Add punctuation, paragraph breaks, and light grammar fixes. Do not translate. Do not summarize away useful details. Do not add facts. Do not force capitalization; preserve natural casing, especially for Indian languages. Return only the final text.";
    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: system_prompt.into(),
            },
            ChatMessage {
                role: "user".into(),
                content: input.to_string(),
            },
        ],
        temperature: 0.0,
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .user_agent("voca/0.1")
        .build()
        .map_err(|e| format!("Failed to create polish client: {}", e))?;

    let mut builder = client.post(endpoint).json(&request);
    if let Some(api_key) = config.polish_api_key() {
        builder = builder.bearer_auth(api_key);
    }

    let response = builder
        .send()
        .await
        .map_err(|e| format!("Polish request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Polish API error ({}): {}", status, body));
    }

    let parsed: ChatResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse polish response: {}", e))?;

    let polished = parsed
        .choices
        .first()
        .map(|choice| choice.message.content.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(input);

    Ok(light_polish(polished))
}

fn apply_voice_commands(input: &str) -> String {
    let words = input.split_whitespace().collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut index = 0;

    while index < words.len() {
        let current = normalize_command_word(words[index]);
        let next = words
            .get(index + 1)
            .map(|word| normalize_command_word(word));

        match (current.as_str(), next.as_deref()) {
            ("new", Some("paragraph")) => {
                output.push("\n\n".to_string());
                index += 2;
            }
            ("new", Some("line")) => {
                output.push("\n".to_string());
                index += 2;
            }
            ("full", Some("stop")) => {
                output.push(".".to_string());
                index += 2;
            }
            ("question", Some("mark")) => {
                output.push("?".to_string());
                index += 2;
            }
            ("exclamation", Some("mark")) => {
                output.push("!".to_string());
                index += 2;
            }
            ("comma", _) => {
                output.push(",".to_string());
                index += 1;
            }
            ("period", _) => {
                output.push(".".to_string());
                index += 1;
            }
            ("colon", _) => {
                output.push(":".to_string());
                index += 1;
            }
            ("semicolon", _) => {
                output.push(";".to_string());
                index += 1;
            }
            _ => {
                output.push(words[index].to_string());
                index += 1;
            }
        }
    }

    output.join(" ")
}

fn remove_disfluencies(input: &str) -> String {
    let words = input.split_whitespace().collect::<Vec<_>>();
    let mut output: Vec<String> = Vec::new();
    let mut index = 0;

    while index < words.len() {
        if matches_filler_phrase(&words, index, &["you", "know"])
            || matches_filler_phrase(&words, index, &["i", "mean"])
            || matches_filler_phrase(&words, index, &["sort", "of"])
            || matches_filler_phrase(&words, index, &["kind", "of"])
        {
            index += 2;
            continue;
        }

        let current = normalize_disfluency_word(words[index]);
        let next = words
            .get(index + 1)
            .map(|word| normalize_disfluency_word(word));

        if is_filler_word(&current) {
            index += 1;
            continue;
        }

        if next.as_deref() == Some(current.as_str()) {
            if is_vague_repeated_word(&current) {
                index += 2;
                continue;
            }

            output.push(words[index].to_string());
            index += 2;
            continue;
        }

        output.push(words[index].to_string());
        index += 1;
    }

    output.join(" ")
}

fn matches_filler_phrase(words: &[&str], index: usize, phrase: &[&str]) -> bool {
    if index + phrase.len() > words.len() {
        return false;
    }

    phrase
        .iter()
        .enumerate()
        .all(|(offset, expected)| normalize_disfluency_word(words[index + offset]) == *expected)
}

fn normalize_disfluency_word(input: &str) -> String {
    input
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '.' | ',' | '?' | '!' | ':' | ';' | '"' | '\'' | DANDA | '(' | ')' | '[' | ']'
            )
        })
        .to_lowercase()
}

fn is_filler_word(word: &str) -> bool {
    matches!(
        word,
        "uh" | "um"
            | "umm"
            | "uhh"
            | "hmm"
            | "hmmm"
            | "er"
            | "ah"
            | "like"
            | "basically"
            | "actually"
            | "literally"
            | "okay"
            | "ok"
            | "so"
            | "matlab"
            | "मतलब"
            | "तो"
            | "आम"
            | "आमेले"
            | "आमेला"
            | "ಆಮೇಲೆ"
            | "ಆಮೆಲೆ"
            | "ಅಮೇಲೆ"
            | "ಮತ್ತೆ"
            | "ಅಂದ್ರೆ"
            | "ಅಂದರೆ"
            | "ಸರೀ"
            | "ಸರಿ"
    )
}

fn is_vague_repeated_word(word: &str) -> bool {
    matches!(
        word,
        "this"
            | "that"
            | "these"
            | "those"
            | "ಇದು"
            | "ಅದು"
            | "ಇವು"
            | "ಅವು"
            | "ये"
            | "वो"
            | "यह"
            | "वह"
    )
}

fn normalize_command_word(input: &str) -> String {
    input
        .trim_matches(|ch: char| matches!(ch, '.' | ',' | '?' | '!' | ':' | ';' | '"' | '\''))
        .to_ascii_lowercase()
}

fn collapse_spaces_preserving_breaks(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut pending_space = false;
    let mut previous_was_break = false;

    for ch in input.chars() {
        if ch == '\n' {
            while output.ends_with(' ') {
                output.pop();
            }
            output.push('\n');
            pending_space = false;
            previous_was_break = true;
        } else if ch.is_whitespace() {
            pending_space = !previous_was_break;
        } else {
            if pending_space && !output.is_empty() {
                output.push(' ');
            }
            output.push(ch);
            pending_space = false;
            previous_was_break = false;
        }
    }

    output
}

fn remove_space_before_punctuation(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '.' | ',' | '?' | '!' | ':' | ';' | DANDA) {
            while output.ends_with(' ') {
                output.pop();
            }
        }
        output.push(ch);
    }
    output
}

fn add_space_after_punctuation(input: &str) -> String {
    let chars = input.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(input.len());

    for (index, ch) in chars.iter().enumerate() {
        output.push(*ch);
        if matches!(*ch, '.' | ',' | '?' | '!' | ':' | ';' | DANDA) {
            if let Some(next) = chars.get(index + 1) {
                if !next.is_whitespace()
                    && !matches!(*next, '.' | ',' | '?' | '!' | ':' | ';' | DANDA)
                {
                    output.push(' ');
                }
            }
        }
    }

    output
}

fn finish_sentence(input: &str) -> String {
    if input.is_empty() || input.ends_with(['.', '?', '!', DANDA]) {
        input.to_string()
    } else if input.chars().any(|ch| ch.is_ascii_alphabetic()) {
        format!("{}.", input)
    } else {
        format!("{}{}", input, DANDA)
    }
}

#[cfg(test)]
mod tests {
    use super::light_polish;

    #[test]
    fn light_polish_keeps_casing_and_fixes_spacing() {
        assert_eq!(
            light_polish("hello   world ,this is  saaras"),
            "hello world, this is saaras."
        );
    }

    #[test]
    fn light_polish_handles_voice_punctuation_commands() {
        assert_eq!(
            light_polish("hello comma this is a test full stop new paragraph next line"),
            "hello, this is a test.\n\nnext line."
        );
    }

    #[test]
    fn light_polish_uses_danda_for_non_latin_text() {
        assert_eq!(
            light_polish("\u{0928}\u{092e}\u{0938}\u{094d}\u{0924}\u{0947}   \u{0926}\u{0941}\u{0928}\u{093f}\u{092f}\u{093e}"),
            "\u{0928}\u{092e}\u{0938}\u{094d}\u{0924}\u{0947} \u{0926}\u{0941}\u{0928}\u{093f}\u{092f}\u{093e}\u{0964}"
        );
    }

    #[test]
    fn light_polish_removes_spoken_disfluencies() {
        assert_eq!(
            light_polish("um i need flowers like and fruits actually"),
            "i need flowers and fruits."
        );
        assert_eq!(
            light_polish("\u{0c86}\u{0cae}\u{0cc7}\u{0cb2}\u{0cc6} \u{0c87}\u{0ca6}\u{0cc1} \u{0c87}\u{0ca6}\u{0cc1} \u{0cb9}\u{0ca3}\u{0ccd}\u{0ca3}\u{0cc1} \u{0ca4}\u{0c97}\u{0ccb}\u{0cac}\u{0cc7}\u{0c95}\u{0cc1}"),
            "\u{0cb9}\u{0ca3}\u{0ccd}\u{0ca3}\u{0cc1} \u{0ca4}\u{0c97}\u{0ccb}\u{0cac}\u{0cc7}\u{0c95}\u{0cc1}\u{0964}"
        );
    }
}
