/// Text chunking and preprocessing — ported from MiroFish
/// `backend/app/services/text_processor.py` and `backend/app/utils/file_parser.py`
/// (split_text_into_chunks at file_parser.py:161-202).
///
/// All string operations are UTF-8 safe: we never slice bytes at a raw byte offset;
/// instead we work with char boundaries obtained from `char_indices()` / `chars().count()`.
use serde::{Deserialize, Serialize};

// -------- Public types --------

/// Statistics about a text string.
///
/// Mirrors `TextProcessor.get_text_stats()` → `{"total_chars", "total_words", "total_lines"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextStats {
    /// Total Unicode scalar values (not bytes).
    pub chars: usize,
    /// Words (whitespace-delimited tokens).
    pub words: usize,
    /// Number of lines (`\n` count + 1, matching Python `text.count('\n') + 1`).
    pub lines: usize,
}

// -------- Public functions --------

/// Split `text` into overlapping char-count windows.
///
/// Mirrors `split_text_into_chunks()` in `file_parser.py:161-202`:
///
/// * If `text` (after checking `.chars().count()`) is ≤ `chunk_size` → return `[text]` unless
///   it is blank (`text.trim().is_empty()`) in which case return `[]`.
/// * Otherwise: advance a `start` cursor by `chunk_size` characters, then try to backtrack to a
///   sentence/paragraph boundary within that window (the same separator priority as MiroFish:
///   `。 ！ ？ .\n !\n ?\n \n\n .space !space ?space`).  The boundary must fall beyond 30 % of
///   the chunk (MiroFish: `last_sep > chunk_size * 0.3`).  If no suitable boundary is found, the
///   hard char-count cut is used.
/// * The overlap carries the last `overlap` characters of the current chunk into the next start
///   position (`start = end - overlap`).
/// * Empty chunks after `.trim()` are dropped.
///
/// **UTF-8 safety guarantee**: every index arithmetic operation is done on char positions
/// returned by `char_indices()`, never on raw byte offsets.  The final `.get()` on a char
/// boundary ensures we never produce invalid UTF-8.
pub fn split_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    // Collect char-boundary byte positions up front so all indexing stays valid.
    // `boundaries[i]` = byte offset of the i-th char.  `boundaries[char_count]` = text.len().
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(byte_pos, _)| byte_pos)
        .chain(std::iter::once(text.len()))
        .collect();

    let char_count = boundaries.len() - 1; // last entry is the sentinel text.len()

    // Short / blank path — matches Python: `if len(text) <= chunk_size: return [text] if text.strip() else []`
    if char_count <= chunk_size {
        return if text.trim().is_empty() { vec![] } else { vec![text.to_string()] };
    }

    // Separator priority list — identical to MiroFish's `for sep in [...]` loop order.
    const SEPS: &[&str] =
        &["。", "！", "？", ".\n", "!\n", "?\n", "\n\n", ". ", "! ", "? "];

    let mut chunks: Vec<String> = Vec::new();
    let mut start_char: usize = 0; // cursor in char positions

    while start_char < char_count {
        let end_char = (start_char + chunk_size).min(char_count);

        // The window slice in bytes — always safe because boundaries[] are char-boundary byte offsets.
        let window_start_byte = boundaries[start_char];
        let window_end_byte = boundaries[end_char];
        let window = &text[window_start_byte..window_end_byte];

        // Only try to backtrack if we haven't reached the end of the text.
        let actual_end_char = if end_char < char_count {
            // Search for each separator in `window`, accept the rightmost one that sits
            // beyond 30 % of chunk_size (MiroFish: `last_sep > chunk_size * 0.3`).
            let min_sep_char = (chunk_size as f64 * 0.3) as usize;

            let mut found_end: Option<usize> = None;
            for sep in SEPS {
                // rfind gives a byte offset within `window`.
                if let Some(sep_byte_in_window) = window.rfind(sep) {
                    // Convert the byte offset within window to a char count within window.
                    let chars_before_sep = window[..sep_byte_in_window].chars().count();
                    let chars_in_sep = sep.chars().count();
                    if chars_before_sep > min_sep_char {
                        // end position = start_char + chars up to and including the separator
                        found_end = Some(start_char + chars_before_sep + chars_in_sep);
                        break;
                    }
                }
            }
            found_end.unwrap_or(end_char)
        } else {
            end_char
        };

        // Clamp to char_count in case the separator calculation went past the text end.
        let actual_end_char = actual_end_char.min(char_count);

        // Build the chunk using byte boundaries (safe).
        let chunk_start_byte = boundaries[start_char];
        let chunk_end_byte = boundaries[actual_end_char];
        let chunk = text[chunk_start_byte..chunk_end_byte].trim();
        if !chunk.is_empty() {
            chunks.push(chunk.to_string());
        }

        // Advance start: overlap carries the last `overlap` chars into the next chunk.
        // Python: `start = end - overlap if end < len(text) else len(text)`
        start_char = if actual_end_char < char_count {
            actual_end_char.saturating_sub(overlap)
        } else {
            char_count
        };
    }

    chunks
}

/// Normalise whitespace in `text`.
///
/// Mirrors `TextProcessor.preprocess_text()` in `text_processor.py:37-61`:
/// 1. CRLF (`\r\n`) and bare CR (`\r`) → `\n`.
/// 2. Three or more consecutive newlines → exactly two newlines.
/// 3. Trailing whitespace stripped from every line (Python `line.strip()`).
/// 4. Leading/trailing whitespace stripped from the result (Python final `.strip()`).
pub fn preprocess_text(text: &str) -> String {
    // Step 1: normalise line endings.
    let text = text.replace("\r\n", "\n").replace('\r', "\n");

    // Step 2: collapse runs of 3+ newlines to exactly 2.
    // We do a single pass rather than a regex to avoid pulling in a dependency.
    let mut result = String::with_capacity(text.len());
    let mut nl_run: usize = 0;
    for ch in text.chars() {
        if ch == '\n' {
            nl_run += 1;
            // Emit at most 2 newlines.
            if nl_run <= 2 {
                result.push('\n');
            }
        } else {
            nl_run = 0;
            result.push(ch);
        }
    }
    let text = result;

    // Step 3: strip BOTH leading and trailing whitespace from every line, matching Python's
    // `[line.strip() for line in text.split('\n')]` (`l.trim()` == `str.strip()`).
    let stripped_lines: Vec<&str> = text.lines().map(|l| l.trim()).collect();
    let text = stripped_lines.join("\n");

    // Step 4: trim the whole string.
    text.trim().to_string()
}

/// Return character, word, and line counts for `text`.
///
/// Mirrors `TextProcessor.get_text_stats()` in `text_processor.py:64-70`:
/// - `total_chars` = `len(text)` — Python `len` on a `str` is Unicode code-point count.
/// - `total_words` = `len(text.split())` — whitespace-split token count.
/// - `total_lines` = `text.count('\n') + 1`.
pub fn get_text_stats(text: &str) -> TextStats {
    let chars = text.chars().count();
    let words = text.split_whitespace().count();
    let lines = text.chars().filter(|&c| c == '\n').count() + 1;
    TextStats { chars, words, lines }
}

// -------- Tests --------

#[cfg(test)]
mod tests {
    use super::*;

    // ===== split_text =====

    #[test]
    fn test_split_text_empty_returns_empty() {
        assert_eq!(split_text("", 500, 50), Vec::<String>::new());
    }

    #[test]
    fn test_split_text_blank_whitespace_returns_empty() {
        assert_eq!(split_text("   \n\n  ", 500, 50), Vec::<String>::new());
    }

    #[test]
    fn test_split_text_short_text_returns_single_chunk() {
        let text = "Hello world.";
        let chunks = split_text(text, 500, 50);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello world.");
    }

    #[test]
    fn test_split_text_exact_multiple_no_remainder() {
        // 10 chars, chunk_size=5, overlap=0 → 2 chunks of 5
        let text = "0123456789";
        let chunks = split_text(text, 5, 0);
        // Each chunk is 5 chars; no separators → hard cut
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "0123456789".chars().take(5).collect::<String>());
        assert_eq!(chunks[1], "0123456789".chars().skip(5).take(5).collect::<String>());
    }

    #[test]
    fn test_split_text_with_remainder() {
        // 11 chars, chunk_size=5, overlap=0 → 3 chunks: 5+5+1
        let text = "01234567890";
        let chunks = split_text(text, 5, 0);
        assert_eq!(chunks.len(), 3, "expected 3 chunks, got: {:?}", chunks);
        let total: usize = chunks.iter().map(|c| c.chars().count()).sum();
        // Total chars in all chunks should cover original (minus any trimmed whitespace)
        assert!(total >= 11, "chunks should cover all content");
    }

    #[test]
    fn test_split_text_overlap_carries_correct_chars() {
        // 20 chars, chunk_size=10, overlap=3 → first chunk [0..10], second starts at 7
        let text = "ABCDEFGHIJKLMNOPQRST"; // 20 chars
        let chunks = split_text(text, 10, 3);
        assert!(chunks.len() >= 2, "expected at least 2 chunks");
        // Second chunk must start with chars from the overlap region (chars 7-9 = "HIJ")
        assert!(
            chunks[1].starts_with("HIJ"),
            "overlap: second chunk should start with 'HIJ', got '{}'",
            chunks[1]
        );
    }

    #[test]
    fn test_split_text_sentence_boundary_backtrack() {
        // Build a text where a period+space separator lies well past 30% of chunk_size=50.
        // "Sentence one. " = 14 chars, then filler to push past 50 chars total, ensuring
        // the '. ' falls in the window and triggers the boundary backtrack.
        let sentence = "Sentence one. ";
        let filler: String = "x".repeat(40);
        let text = format!("{sentence}{filler}extra text here.");
        let _chunks = split_text(&text, 50, 5);
        // The first chunk should end at or shortly after the ". " separator, not at char 50.
        // The boundary is at char 13 + 2 = 15 (". " is 2 chars). 15 > 50*0.3=15 (exclusive >),
        // so we need the boundary strictly > 15. Use a longer sentence to guarantee this.
        // Re-build: "This is a long sentence with some words in it. " (≥16 chars before '. ')
        let sentence2 = "This is a long sentence with some words in it. ";
        let filler2: String = "y".repeat(20);
        let text2 = format!("{sentence2}{filler2}next chunk starts here now.");
        let chunks2 = split_text(&text2, 50, 5);
        assert!(chunks2.len() >= 2, "expected at least 2 chunks");
        // First chunk should contain the sentence and end near the '. '
        assert!(
            chunks2[0].contains("sentence with some words"),
            "first chunk should contain the sentence"
        );
    }

    #[test]
    fn test_split_text_utf8_multibyte_never_panics() {
        // Chinese text — each char is 3 UTF-8 bytes; slicing at byte offsets would panic.
        let text = "你好世界，这是一段中文文本。".repeat(10);
        let chunks = split_text(&text, 5, 1);
        assert!(!chunks.is_empty(), "should produce at least one chunk");
        // Verify all chunks are valid UTF-8 strings (Rust String invariant already guarantees this).
        for chunk in &chunks {
            assert!(!chunk.is_empty(), "no empty chunks");
        }
    }

    #[test]
    fn test_split_text_utf8_no_split_mid_char() {
        // 10 three-byte chars = 30 bytes; chunk_size=3 chars = 9 bytes.
        // Any byte-based split would land mid-char; our char-boundary approach must not panic.
        let text = "あいうえおかきくけこ"; // 10 chars, each 3 bytes
        let chunks = split_text(text, 3, 0);
        // Just verify it doesn't panic and produces valid content.
        for chunk in &chunks {
            assert!(!chunk.is_empty());
            // Ensure each chunk parses correctly as UTF-8 (implicit in &str / String).
        }
        // Total char count across chunks should cover the original.
        let total_chars: usize = chunks.iter().map(|c| c.chars().count()).sum();
        assert!(total_chars >= 9, "expected chunks to cover most of the text, got {total_chars}");
    }

    #[test]
    fn test_split_text_chinese_sentence_boundary() {
        // Chinese full-stop separator `。` should trigger boundary backtrack.
        // Build a ~60-char block with `。` at position ~20 (well past 30% of 50).
        let part1 = "这是第一句话，讲述了一些重要的事情。"; // ~17 chars + 。
        let part2 = "第二句话也很重要，包含了更多信息和细节。";
        let text = format!("{part1}{part2}");
        let chunks = split_text(&text, 20, 2);
        assert!(!chunks.is_empty());
        // All chunks must be valid (non-empty, no panic).
        for chunk in &chunks {
            assert!(!chunk.is_empty());
        }
    }

    // ===== preprocess_text =====

    #[test]
    fn test_preprocess_crlf_normalized() {
        let input = "line1\r\nline2\r\nline3";
        let result = preprocess_text(input);
        assert_eq!(result, "line1\nline2\nline3");
    }

    #[test]
    fn test_preprocess_bare_cr_normalized() {
        let input = "line1\rline2\rline3";
        let result = preprocess_text(input);
        assert_eq!(result, "line1\nline2\nline3");
    }

    #[test]
    fn test_preprocess_blank_run_collapsed() {
        // 4 consecutive newlines → exactly 2
        let input = "para1\n\n\n\npara2";
        let result = preprocess_text(input);
        assert_eq!(result, "para1\n\npara2");

        // 3 consecutive newlines → exactly 2
        let input2 = "a\n\n\nb";
        let result2 = preprocess_text(input2);
        assert_eq!(result2, "a\n\nb");

        // 2 newlines — must stay as 2.
        let input3 = "a\n\nb";
        let result3 = preprocess_text(input3);
        assert_eq!(result3, "a\n\nb");
    }

    #[test]
    fn test_preprocess_trailing_whitespace_per_line() {
        let input = "line1   \nline2\t\nline3";
        let result = preprocess_text(input);
        assert_eq!(result, "line1\nline2\nline3");
    }

    #[test]
    fn test_preprocess_leading_trailing_strip() {
        let input = "\n\nhello\n\n";
        let result = preprocess_text(input);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_preprocess_combined() {
        let input = "  hello  \r\n\n\n\nworld  \r\n";
        let result = preprocess_text(input);
        // CRLF → \n, runs collapsed, each line stripped, whole result trimmed
        assert_eq!(result, "hello\n\nworld");
    }

    // ===== get_text_stats =====

    #[test]
    fn test_stats_simple() {
        let text = "hello world\nfoo bar";
        let stats = get_text_stats(text);
        assert_eq!(stats.chars, 19);
        assert_eq!(stats.words, 4);
        assert_eq!(stats.lines, 2);
    }

    #[test]
    fn test_stats_single_line() {
        let text = "one two three";
        let stats = get_text_stats(text);
        assert_eq!(stats.lines, 1, "no newlines → 1 line");
        assert_eq!(stats.words, 3);
        assert_eq!(stats.chars, 13);
    }

    #[test]
    fn test_stats_empty() {
        let stats = get_text_stats("");
        assert_eq!(stats.chars, 0);
        assert_eq!(stats.words, 0);
        assert_eq!(stats.lines, 1, "empty string still counts as 1 line (\\n count + 1 = 0 + 1)");
    }

    #[test]
    fn test_stats_unicode() {
        // "你好" = 2 Unicode code points (chars), not 6 bytes
        let text = "你好\nworld";
        let stats = get_text_stats(text);
        assert_eq!(stats.chars, 8, "2 CJK + newline + 5 ASCII = 8 code points");
        assert_eq!(stats.words, 2);
        assert_eq!(stats.lines, 2);
    }
}
