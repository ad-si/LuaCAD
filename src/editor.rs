#[derive(Debug, Clone)]
pub enum EditorAction {
  SelectNextOccurrence, // Cmd+D
  SelectLine,           // Cmd+L
  ToggleComment,        // Cmd+G (Cmd+/ blocked by three-d#571)
  InsertTab,            // Tab — insert 2 spaces or indent selected lines
  Unindent,             // Shift+Tab — unindent selected lines
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
