#[derive(Debug, Clone)]
pub enum EditorAction {
  SelectNextOccurrence,   // Cmd+D
  SelectLine,             // Cmd+L
  ToggleComment,          // Cmd+/
  InsertTab,              // Tab — insert 2 spaces or indent selected lines
  Unindent,               // Shift+Tab — unindent selected lines
  PasteLineAbove(String), // Paste whole-line clipboard above the current line
  CutLine,                // Cmd+X with no selection — cut the whole line
  DeleteCharRight,        // Ctrl+D — delete character right of cursor
  DeleteWordLeft,         // Ctrl+W — delete word left of cursor
  WrapSelection(char),    // Wrap selected text with bracket pair: (, [, {
}

/// How a character is grouped when a click expands a selection.
#[derive(PartialEq, Eq, Clone, Copy)]
enum CharClass {
  Word,
  Space,
  Newline,
  Punctuation,
}

fn char_class(c: char) -> CharClass {
  if c == '\n' || c == '\r' {
    CharClass::Newline
  } else if c.is_alphanumeric() || c == '_' {
    CharClass::Word
  } else if c.is_whitespace() {
    CharClass::Space
  } else {
    CharClass::Punctuation
  }
}

/// Byte offset of the `char_idx`-th character.
fn byte_index_of(text: &str, char_idx: usize) -> usize {
  text
    .char_indices()
    .nth(char_idx)
    .map_or(text.len(), |(byte_idx, _)| byte_idx)
}

/// The character range a double click selects around `caret`: the run of
/// like characters it sits in — a word, a stretch of spaces, a stretch of
/// punctuation.
///
/// This only walks outwards from the caret. egui's own `select_word_at`
/// re-scans (and, for the left boundary, reverses) the entire buffer for
/// every lookup, which makes double clicking a word in a large file take
/// seconds.
pub fn double_click_range(text: &str, caret: usize) -> (usize, usize) {
  let byte_idx = byte_index_of(text, caret);
  let before = text[..byte_idx].chars().next_back();
  let after = text[byte_idx..].chars().next();

  // A caret right after a word belongs to that word, like in native editors
  let class = match (before, after) {
    (_, Some(c)) if char_class(c) == CharClass::Word => CharClass::Word,
    (Some(c), _) if char_class(c) == CharClass::Word => CharClass::Word,
    (_, Some(c)) => char_class(c),
    (Some(c), _) => char_class(c),
    (None, None) => return (caret, caret),
  };

  let same_class = |c: &char| char_class(*c) == class;
  let left = text[..byte_idx]
    .chars()
    .rev()
    .take_while(same_class)
    .count();
  let right = text[byte_idx..].chars().take_while(same_class).count();
  (caret - left, caret + right)
}

/// The character range a triple click selects: the line around `caret`,
/// without its line break.
pub fn triple_click_range(text: &str, caret: usize) -> (usize, usize) {
  let byte_idx = byte_index_of(text, caret);
  let not_newline = |c: &char| *c != '\n';
  let left = text[..byte_idx]
    .chars()
    .rev()
    .take_while(not_newline)
    .count();
  let right = text[byte_idx..].chars().take_while(not_newline).count();
  (caret - left, caret + right)
}

/// Get the word boundaries around a character index in the text.
/// Returns (start, end) character indices of the word.
fn word_at(text: &str, char_idx: usize) -> (usize, usize) {
  let chars: Vec<char> = text.chars().collect();
  if char_idx >= chars.len() {
    return (char_idx, char_idx);
  }

  let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

  if !is_word_char(chars[char_idx]) {
    return (char_idx, char_idx + 1);
  }

  let mut start = char_idx;
  while start > 0 && is_word_char(chars[start - 1]) {
    start -= 1;
  }

  let mut end = char_idx;
  while end < chars.len() && is_word_char(chars[end]) {
    end += 1;
  }

  (start, end)
}

/// Find the line start and end (including trailing newline) for the line containing `char_idx`.
fn line_range_at(text: &str, char_idx: usize) -> (usize, usize) {
  let chars: Vec<char> = text.chars().collect();
  let idx = char_idx.min(chars.len().saturating_sub(1));

  let mut start = idx;
  while start > 0 && chars[start - 1] != '\n' {
    start -= 1;
  }

  let mut end = idx;
  while end < chars.len() && chars[end] != '\n' {
    end += 1;
  }
  // Include the trailing newline if present
  if end < chars.len() && chars[end] == '\n' {
    end += 1;
  }

  (start, end)
}

/// The line a whole-line copy or cut of Cmd+C / Cmd+X takes, as char indices
/// including the trailing newline. Empty when the caret sits on the empty
/// line after a trailing newline — there is no line to take there.
fn whole_line_range(text: &str, cursor: usize) -> (usize, usize) {
  let total = text.chars().count();
  if cursor >= total && text.ends_with('\n') {
    return (total, total);
  }
  line_range_at(text, cursor)
}

/// The text of the line around `cursor` (a char index), including its
/// trailing newline. Empty when there is no line to take.
pub fn whole_line_at(text: &str, cursor: usize) -> String {
  let (start, end) = whole_line_range(text, cursor);
  text.chars().skip(start).take(end - start).collect()
}

/// Apply a pending editor action, returning the new cursor range (as char indices).
pub fn apply_editor_action(
  action: &EditorAction,
  text: &mut String,
  cursor_start: usize,
  cursor_end: usize,
) -> (usize, usize) {
  match action {
    EditorAction::SelectNextOccurrence => {
      if cursor_start == cursor_end {
        // No selection: select the word under cursor
        let (ws, we) = word_at(text, cursor_start);
        (ws, we)
      } else {
        // Has selection: find next occurrence of selected text
        let chars: Vec<char> = text.chars().collect();
        let selected: String = chars[cursor_start..cursor_end].iter().collect();
        let after_selection: String = chars[cursor_end..].iter().collect();

        if let Some(rel_pos) = after_selection.find(&selected) {
          // Convert byte offset from find() to char offset
          let char_offset = after_selection[..rel_pos].chars().count();
          let new_start = cursor_end + char_offset;
          let new_end = new_start + (cursor_end - cursor_start);
          (new_start, new_end)
        } else {
          // Wrap around: search from beginning
          let before_selection: String = chars[..cursor_start].iter().collect();
          if let Some(rel_pos) = before_selection.find(&selected) {
            let char_offset = before_selection[..rel_pos].chars().count();
            let new_end = char_offset + (cursor_end - cursor_start);
            (char_offset, new_end)
          } else {
            // Only one occurrence, keep current selection
            (cursor_start, cursor_end)
          }
        }
      }
    }

    EditorAction::SelectLine => {
      if cursor_start == cursor_end {
        // No selection: select current line
        line_range_at(text, cursor_start)
      } else {
        // Already have selection: extend to include next line
        let (_, end) = line_range_at(text, cursor_end.saturating_sub(1));
        if end < text.chars().count() {
          let (_, next_end) = line_range_at(text, end);
          (cursor_start, next_end)
        } else {
          (cursor_start, end)
        }
      }
    }

    EditorAction::ToggleComment => {
      let chars: Vec<char> = text.chars().collect();
      let total_chars = chars.len();

      // Find all lines that overlap the selection
      let sel_start = cursor_start.min(cursor_end);
      let sel_end = if cursor_start == cursor_end {
        cursor_end
      } else {
        // Don't include a line if selection ends at its very start
        cursor_end.saturating_sub(1)
      };

      // Collect line ranges
      let mut line_ranges: Vec<(usize, usize)> = Vec::new();
      let (first_start, first_end) = line_range_at(text, sel_start);
      line_ranges.push((first_start, first_end));

      let mut pos = first_end;
      while pos <= sel_end && pos < total_chars {
        let (ls, le) = line_range_at(text, pos);
        line_ranges.push((ls, le));
        if le == pos {
          break; // prevent infinite loop
        }
        pos = le;
      }

      // Check if all lines are already commented
      let all_commented = line_ranges.iter().all(|(ls, le)| {
        let line: String = chars[*ls..*le].iter().collect();
        let trimmed = line.trim_start();
        trimmed.starts_with("--") || trimmed.is_empty()
      });

      // Build new text by processing lines in reverse order to maintain char indices
      let mut new_text = text.clone();
      let mut offset: i64 = 0;

      // Process lines front-to-back, tracking the cumulative offset
      for (ls, _le) in &line_ranges {
        let adjusted_start = (*ls as i64 + offset) as usize;
        let line_chars: Vec<char> = new_text.chars().collect();
        // Find the first non-whitespace position in this line
        let mut first_non_ws = adjusted_start;
        while first_non_ws < line_chars.len()
          && line_chars[first_non_ws] != '\n'
          && line_chars[first_non_ws].is_whitespace()
        {
          first_non_ws += 1;
        }

        // Skip empty lines (or lines that are just a newline)
        if first_non_ws >= line_chars.len() || line_chars[first_non_ws] == '\n'
        {
          continue;
        }

        // Convert char index to byte index for string operations
        let byte_idx: usize =
          line_chars[..first_non_ws].iter().collect::<String>().len();

        if all_commented {
          // Remove "-- " or "--"
          if new_text[byte_idx..].starts_with("-- ") {
            new_text.replace_range(byte_idx..byte_idx + 3, "");
            offset -= 3;
          } else if new_text[byte_idx..].starts_with("--") {
            new_text.replace_range(byte_idx..byte_idx + 2, "");
            offset -= 2;
          }
        } else {
          // Add "-- "
          new_text.insert_str(byte_idx, "-- ");
          offset += 3;
        }
      }

      let new_len = new_text.chars().count();
      let new_cursor_end = (cursor_end as i64 + offset).max(0) as usize;
      let new_cursor_end = new_cursor_end.min(new_len);
      let new_cursor_start = if cursor_start == cursor_end {
        new_cursor_end
      } else {
        cursor_start.min(new_len)
      };

      *text = new_text;
      (new_cursor_start, new_cursor_end)
    }

    EditorAction::InsertTab => {
      if cursor_start == cursor_end {
        // No selection: insert 2 spaces at cursor
        let byte_idx: usize =
          text.chars().take(cursor_start).collect::<String>().len();
        text.insert_str(byte_idx, "  ");
        (cursor_start + 2, cursor_start + 2)
      } else {
        // Selection: indent all selected lines by 2 spaces
        let sel_start = cursor_start.min(cursor_end);
        let sel_end = cursor_start.max(cursor_end);
        let mut line_ranges: Vec<(usize, usize)> = Vec::new();
        let total_chars = text.chars().count();
        let (first_start, first_end) = line_range_at(text, sel_start);
        line_ranges.push((first_start, first_end));
        let mut pos = first_end;
        while pos < sel_end && pos < total_chars {
          let (ls, le) = line_range_at(text, pos);
          line_ranges.push((ls, le));
          if le == pos {
            break;
          }
          pos = le;
        }

        let mut new_text = text.clone();
        let mut offset: i64 = 0;
        for (ls, _) in &line_ranges {
          let adjusted = (*ls as i64 + offset) as usize;
          let byte_idx: usize =
            new_text.chars().take(adjusted).collect::<String>().len();
          new_text.insert_str(byte_idx, "  ");
          offset += 2;
        }

        let new_start = cursor_start + 2; // first line always indented
        let new_end = (cursor_end as i64 + offset) as usize;
        *text = new_text;
        (new_start, new_end)
      }
    }

    EditorAction::PasteLineAbove(line_text) => {
      // Insert the whole-line text at the start of the current line
      let (line_start, _) = line_range_at(text, cursor_start);
      let byte_idx: usize =
        text.chars().take(line_start).collect::<String>().len();
      let insert = if line_text.ends_with('\n') {
        line_text.clone()
      } else {
        format!("{}\n", line_text)
      };
      let insert_chars = insert.chars().count();
      text.insert_str(byte_idx, &insert);
      // Keep cursor at its original position (shifted by inserted text), no selection
      let new_pos = cursor_start + insert_chars;
      (new_pos, new_pos)
    }

    EditorAction::CutLine => {
      let (start, end) = whole_line_range(text, cursor_start);
      if start == end {
        return (cursor_start, cursor_end);
      }
      let chars: Vec<char> = text.chars().collect();
      let column = cursor_start.saturating_sub(start);
      let byte_start: usize = chars[..start].iter().collect::<String>().len();
      let byte_end: usize = chars[..end].iter().collect::<String>().len();
      text.replace_range(byte_start..byte_end, "");

      // Keep the caret in its column on the line that moved up
      let line_len =
        text.chars().skip(start).take_while(|c| *c != '\n').count();
      let new_pos = (start + column.min(line_len)).min(text.chars().count());
      (new_pos, new_pos)
    }

    EditorAction::DeleteCharRight => {
      let chars: Vec<char> = text.chars().collect();
      if cursor_start == cursor_end && cursor_start < chars.len() {
        let byte_start: usize =
          chars[..cursor_start].iter().collect::<String>().len();
        let byte_end: usize =
          chars[..cursor_start + 1].iter().collect::<String>().len();
        text.replace_range(byte_start..byte_end, "");
      }
      (cursor_start, cursor_start)
    }

    EditorAction::DeleteWordLeft => {
      if cursor_start == cursor_end && cursor_start > 0 {
        let chars: Vec<char> = text.chars().collect();
        let mut pos = cursor_start;

        // Skip whitespace backwards first
        while pos > 0
          && chars[pos - 1].is_whitespace()
          && chars[pos - 1] != '\n'
        {
          pos -= 1;
        }

        // Then delete the word (or single non-word char)
        if pos > 0 {
          if chars[pos - 1].is_alphanumeric() || chars[pos - 1] == '_' {
            while pos > 0
              && (chars[pos - 1].is_alphanumeric() || chars[pos - 1] == '_')
            {
              pos -= 1;
            }
          } else {
            pos -= 1;
          }
        }

        let byte_start: usize = chars[..pos].iter().collect::<String>().len();
        let byte_end: usize =
          chars[..cursor_start].iter().collect::<String>().len();
        text.replace_range(byte_start..byte_end, "");
        (pos, pos)
      } else if cursor_start != cursor_end {
        // Delete selection
        let start = cursor_start.min(cursor_end);
        let end = cursor_start.max(cursor_end);
        let chars: Vec<char> = text.chars().collect();
        let byte_start: usize = chars[..start].iter().collect::<String>().len();
        let byte_end: usize = chars[..end].iter().collect::<String>().len();
        text.replace_range(byte_start..byte_end, "");
        (start, start)
      } else {
        (cursor_start, cursor_end)
      }
    }

    EditorAction::WrapSelection(open) => {
      let close = match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => *open,
      };
      let start = cursor_start.min(cursor_end);
      let end = cursor_start.max(cursor_end);
      let chars: Vec<char> = text.chars().collect();

      // Insert closing bracket first (at end) so start index stays valid
      let byte_end: usize = chars[..end].iter().collect::<String>().len();
      text.insert(byte_end, close);
      let byte_start: usize = chars[..start].iter().collect::<String>().len();
      text.insert(byte_start, *open);

      // New selection: inside the brackets (original selection shifted by 1)
      (start + 1, end + 1)
    }

    EditorAction::Unindent => {
      let sel_start = cursor_start.min(cursor_end);
      let sel_end = if cursor_start == cursor_end {
        cursor_end
      } else {
        cursor_start.max(cursor_end)
      };
      let chars: Vec<char> = text.chars().collect();
      let total_chars = chars.len();

      let mut line_ranges: Vec<(usize, usize)> = Vec::new();
      let (first_start, first_end) = line_range_at(text, sel_start);
      line_ranges.push((first_start, first_end));
      let mut pos = first_end;
      while pos < sel_end && pos < total_chars {
        let (ls, le) = line_range_at(text, pos);
        line_ranges.push((ls, le));
        if le == pos {
          break;
        }
        pos = le;
      }

      let mut new_text = text.clone();
      let mut offset: i64 = 0;
      let mut first_line_removed: i64 = 0;
      for (i, (ls, _)) in line_ranges.iter().enumerate() {
        let adjusted = (*ls as i64 + offset) as usize;
        let line_chars: Vec<char> = new_text.chars().collect();
        // Count leading spaces to remove (up to 2)
        let mut spaces = 0;
        while spaces < 2
          && adjusted + spaces < line_chars.len()
          && line_chars[adjusted + spaces] == ' '
        {
          spaces += 1;
        }
        if spaces > 0 {
          let byte_start: usize =
            line_chars[..adjusted].iter().collect::<String>().len();
          let byte_end: usize = line_chars[..adjusted + spaces]
            .iter()
            .collect::<String>()
            .len();
          new_text.replace_range(byte_start..byte_end, "");
          offset -= spaces as i64;
          if i == 0 {
            first_line_removed = spaces as i64;
          }
        }
      }

      let new_start =
        (cursor_start as i64 - first_line_removed).max(0) as usize;
      let new_end = (cursor_end as i64 + offset).max(0) as usize;
      let new_len = new_text.chars().count();
      *text = new_text;
      (new_start.min(new_len), new_end.min(new_len))
    }
  }
}

#[cfg(test)]
mod line_cut_tests {
  use super::{EditorAction, apply_editor_action, whole_line_at};

  /// Cut the line the caret sits on, as Cmd+X does without a selection.
  fn cut(text: &mut String, caret: usize) -> (String, usize) {
    let clipboard = whole_line_at(text, caret);
    let (start, _) =
      apply_editor_action(&EditorAction::CutLine, text, caret, caret);
    (clipboard, start)
  }

  #[test]
  fn cut_takes_the_whole_line_including_its_break() {
    let mut text = "one\ntwo\nthree\n".to_string();
    let (clipboard, caret) = cut(&mut text, 5); // inside `two`
    assert_eq!(clipboard, "two\n");
    assert_eq!(text, "one\nthree\n");
    // Caret keeps its column on the line that moved up
    assert_eq!(caret, 5);
  }

  #[test]
  fn cut_then_paste_restores_the_line() {
    let mut text = "one\ntwo\nthree\n".to_string();
    let (clipboard, caret) = cut(&mut text, 5);
    let (new_caret, _) = apply_editor_action(
      &EditorAction::PasteLineAbove(clipboard),
      &mut text,
      caret,
      caret,
    );
    assert_eq!(text, "one\ntwo\nthree\n");
    assert_eq!(new_caret, 9);
  }

  #[test]
  fn cut_moves_the_line_up_when_pasted_one_line_higher() {
    let mut text = "one\ntwo\nthree\n".to_string();
    // Cut `three`, then paste with the caret on `two`
    let (clipboard, _) = cut(&mut text, 8);
    assert_eq!(text, "one\ntwo\n");
    apply_editor_action(
      &EditorAction::PasteLineAbove(clipboard),
      &mut text,
      5,
      5,
    );
    assert_eq!(text, "one\nthree\ntwo\n");
  }

  #[test]
  fn cut_of_a_blank_line_keeps_the_break() {
    let mut text = "one\n\ntwo\n".to_string();
    let (clipboard, caret) = cut(&mut text, 4);
    assert_eq!(clipboard, "\n");
    assert_eq!(text, "one\ntwo\n");
    assert_eq!(caret, 4);
  }

  #[test]
  fn cut_of_the_last_line_without_a_break() {
    let mut text = "one\ntwo".to_string();
    let (clipboard, caret) = cut(&mut text, 6);
    assert_eq!(clipboard, "two");
    assert_eq!(text, "one\n");
    assert_eq!(caret, 4);
  }

  #[test]
  fn caret_on_the_empty_line_after_a_trailing_break_cuts_nothing() {
    let mut text = "one\ntwo\n".to_string();
    let (clipboard, caret) = cut(&mut text, 8);
    assert!(clipboard.is_empty());
    assert_eq!(text, "one\ntwo\n");
    assert_eq!(caret, 8);
  }

  #[test]
  fn caret_column_is_clamped_to_a_shorter_following_line() {
    let mut text = "aaaaaaaa\nbb\n".to_string();
    let (_, caret) = cut(&mut text, 6);
    assert_eq!(text, "bb\n");
    assert_eq!(caret, 2);
  }

  #[test]
  fn multi_byte_characters_keep_char_indices_aligned() {
    let mut text = "größe\nwidth\n".to_string();
    let (clipboard, _) = cut(&mut text, 2);
    assert_eq!(clipboard, "größe\n");
    assert_eq!(text, "width\n");
  }
}

#[cfg(test)]
mod click_range_tests {
  use super::{double_click_range, triple_click_range};

  /// Character index of the first occurrence of `needle` in `text`.
  fn caret_at(text: &str, needle: &str) -> usize {
    text[..text.find(needle).expect("needle in text")]
      .chars()
      .count()
  }

  fn selected(text: &str, caret: usize) -> &str {
    let (start, end) = double_click_range(text, caret);
    let byte_start = super::byte_index_of(text, start);
    let byte_end = super::byte_index_of(text, end);
    &text[byte_start..byte_end]
  }

  #[test]
  fn selects_the_word_around_the_caret() {
    let text = "local width = 10";
    let w = caret_at(text, "width");
    assert_eq!(selected(text, w), "width");
    assert_eq!(selected(text, w + 2), "width");
    // Caret at the trailing edge still belongs to the word
    assert_eq!(selected(text, w + 5), "width");
  }

  #[test]
  fn selects_runs_of_spaces_and_punctuation() {
    let text = "f(a,   b) -- ok";
    assert_eq!(selected(text, caret_at(text, "   ") + 1), "   ");
    assert_eq!(selected(text, caret_at(text, "--") + 1), "--");
    // A caret between a word and punctuation belongs to the word
    assert_eq!(selected(text, caret_at(text, ",")), "a");
  }

  #[test]
  fn does_not_cross_line_breaks() {
    let text = "alpha\nbeta";
    assert_eq!(selected(text, caret_at(text, "beta")), "beta");
    // Caret right after `alpha`, before the newline
    assert_eq!(selected(text, 5), "alpha");
  }

  #[test]
  fn word_with_underscores_and_digits_stays_one_word() {
    let text = "tooth_width_2 = 3";
    assert_eq!(selected(text, 4), "tooth_width_2");
  }

  #[test]
  fn triple_click_takes_the_line_without_the_break() {
    let text = "local a = 1\nlocal b = 2\n";
    let caret = caret_at(text, "b");
    let (start, end) = triple_click_range(text, caret);
    let chars: String = text.chars().skip(start).take(end - start).collect();
    assert_eq!(chars, "local b = 2");
  }

  #[test]
  fn handles_empty_text_and_end_of_text() {
    assert_eq!(double_click_range("", 0), (0, 0));
    let text = "abc";
    assert_eq!(double_click_range(text, 3), (0, 3));
  }
}
