#[derive(Default)]
pub(super) struct ApplyPatchArgumentDeltaNormalizer {
    mode: ApplyPatchArgumentDeltaMode,
}

enum ApplyPatchArgumentDeltaMode {
    Undetected(String),
    Raw,
    JsonString(JsonStringDeltaDecoder),
    JsonObject(JsonObjectInputDecoder),
}

impl Default for ApplyPatchArgumentDeltaMode {
    fn default() -> Self {
        Self::Undetected(String::new())
    }
}

impl ApplyPatchArgumentDeltaNormalizer {
    pub(super) fn push_delta(&mut self, delta: &str) -> Vec<String> {
        match std::mem::take(&mut self.mode) {
            ApplyPatchArgumentDeltaMode::Undetected(mut pending) => {
                pending.push_str(delta);
                let Some(first) = pending.find(|ch: char| !ch.is_whitespace()) else {
                    self.mode = ApplyPatchArgumentDeltaMode::Undetected(pending);
                    return Vec::new();
                };
                let first_char = pending[first..]
                    .chars()
                    .next()
                    .expect("first non-whitespace char");
                match first_char {
                    '"' => {
                        let mut decoder = JsonStringDeltaDecoder::default();
                        let start = first + first_char.len_utf8();
                        let decoded = decoder.push_delta(&pending[start..]);
                        self.mode = ApplyPatchArgumentDeltaMode::JsonString(decoder);
                        non_empty_delta(decoded)
                    }
                    '{' => {
                        let mut decoder = JsonObjectInputDecoder::default();
                        let decoded = decoder.push_delta(&pending[first..]);
                        self.mode = ApplyPatchArgumentDeltaMode::JsonObject(decoder);
                        non_empty_delta(decoded)
                    }
                    _ => {
                        self.mode = ApplyPatchArgumentDeltaMode::Raw;
                        non_empty_delta(pending)
                    }
                }
            }
            ApplyPatchArgumentDeltaMode::Raw => {
                self.mode = ApplyPatchArgumentDeltaMode::Raw;
                non_empty_delta(delta.to_string())
            }
            ApplyPatchArgumentDeltaMode::JsonString(mut decoder) => {
                let decoded = decoder.push_delta(delta);
                self.mode = ApplyPatchArgumentDeltaMode::JsonString(decoder);
                non_empty_delta(decoded)
            }
            ApplyPatchArgumentDeltaMode::JsonObject(mut decoder) => {
                let decoded = decoder.push_delta(delta);
                self.mode = ApplyPatchArgumentDeltaMode::JsonObject(decoder);
                non_empty_delta(decoded)
            }
        }
    }
}

fn non_empty_delta(delta: String) -> Vec<String> {
    if delta.is_empty() {
        Vec::new()
    } else {
        vec![delta]
    }
}

#[derive(Default)]
struct JsonObjectInputDecoder {
    prefix: String,
    input_decoder: Option<JsonStringDeltaDecoder>,
}

impl JsonObjectInputDecoder {
    fn push_delta(&mut self, delta: &str) -> String {
        if let Some(decoder) = &mut self.input_decoder {
            return decoder.push_delta(delta);
        }

        self.prefix.push_str(delta);
        let Some(start) = find_input_string_start(&self.prefix) else {
            truncate_to_last_chars(&mut self.prefix, 128);
            return String::new();
        };

        let remainder = self.prefix[start..].to_string();
        self.prefix.clear();
        let mut decoder = JsonStringDeltaDecoder::default();
        let decoded = decoder.push_delta(&remainder);
        self.input_decoder = Some(decoder);
        decoded
    }
}

fn find_input_string_start(input: &str) -> Option<usize> {
    let mut search_start = 0;
    while let Some(relative_key_start) = input[search_start..].find("\"input\"") {
        let key_start = search_start + relative_key_start;
        let mut cursor = key_start + "\"input\"".len();
        cursor = skip_json_whitespace(input, cursor);
        if !input[cursor..].starts_with(':') {
            search_start = key_start + 1;
            continue;
        }
        cursor += ':'.len_utf8();
        cursor = skip_json_whitespace(input, cursor);
        if input[cursor..].starts_with('"') {
            return Some(cursor + '"'.len_utf8());
        }
        search_start = key_start + 1;
    }
    None
}

fn skip_json_whitespace(input: &str, mut cursor: usize) -> usize {
    for ch in input[cursor..].chars() {
        if !ch.is_whitespace() {
            break;
        }
        cursor += ch.len_utf8();
    }
    cursor
}

fn truncate_to_last_chars(input: &mut String, max_chars: usize) {
    let char_count = input.chars().count();
    if char_count > max_chars {
        *input = input.chars().skip(char_count - max_chars).collect();
    }
}

#[derive(Default)]
struct JsonStringDeltaDecoder {
    escaping: bool,
    unicode_escape: String,
    closed: bool,
}

impl JsonStringDeltaDecoder {
    fn push_delta(&mut self, delta: &str) -> String {
        let mut decoded = String::new();
        for ch in delta.chars() {
            if self.closed {
                break;
            }
            if !self.unicode_escape.is_empty() {
                self.push_unicode_escape_char(ch, &mut decoded);
                continue;
            }
            if self.escaping {
                self.push_escaped_char(ch, &mut decoded);
                continue;
            }
            match ch {
                '\\' => self.escaping = true,
                '"' => self.closed = true,
                ch => decoded.push(ch),
            }
        }
        decoded
    }

    fn push_escaped_char(&mut self, ch: char, decoded: &mut String) {
        match ch {
            '"' => decoded.push('"'),
            '\\' => decoded.push('\\'),
            '/' => decoded.push('/'),
            'b' => decoded.push('\u{0008}'),
            'f' => decoded.push('\u{000c}'),
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            'u' => {
                self.unicode_escape.push('u');
                return;
            }
            ch => decoded.push(ch),
        }
        self.escaping = false;
    }

    fn push_unicode_escape_char(&mut self, ch: char, decoded: &mut String) {
        if ch.is_ascii_hexdigit() {
            self.unicode_escape.push(ch);
            if self.unicode_escape.len() == 5 {
                if let Ok(codepoint) = u32::from_str_radix(&self.unicode_escape[1..], 16)
                    && let Some(ch) = char::from_u32(codepoint)
                {
                    decoded.push(ch);
                }
                self.unicode_escape.clear();
                self.escaping = false;
            }
            return;
        }

        decoded.push_str("\\u");
        decoded.push_str(&self.unicode_escape[1..]);
        decoded.push(ch);
        self.unicode_escape.clear();
        self.escaping = false;
    }
}
