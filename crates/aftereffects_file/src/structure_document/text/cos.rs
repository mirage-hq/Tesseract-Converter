use std::collections::BTreeMap;

#[derive(Debug)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Name(String),
    Array(Vec<Value>),
    Dict(BTreeMap<String, Value>),
}

// Parsing and destruction are iterative; ownership and style comparisons must
// also tolerate the same deeply nested native Source Text containers.
impl Clone for Value {
    fn clone(&self) -> Self {
        let mut pending = vec![(self, false)];
        let mut cloned = Vec::new();
        while let Some((value, visited)) = pending.pop() {
            match value {
                Self::Array(values) if !visited => {
                    pending.push((value, true));
                    pending.extend(values.iter().rev().map(|value| (value, false)));
                }
                Self::Dict(values) if !visited => {
                    pending.push((value, true));
                    pending.extend(values.values().rev().map(|value| (value, false)));
                }
                Self::Array(values) => {
                    let children = cloned.split_off(cloned.len() - values.len());
                    cloned.push(Self::Array(children));
                }
                Self::Dict(values) => {
                    let children = cloned.split_off(cloned.len() - values.len());
                    cloned.push(Self::Dict(values.keys().cloned().zip(children).collect()));
                }
                Self::Null => cloned.push(Self::Null),
                Self::Bool(value) => cloned.push(Self::Bool(*value)),
                Self::Number(value) => cloned.push(Self::Number(*value)),
                Self::String(value) => cloned.push(Self::String(value.clone())),
                Self::Name(value) => cloned.push(Self::Name(value.clone())),
            }
        }
        cloned.pop().expect("the root value was cloned")
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (Self::Null, Self::Null) => {}
                (Self::Bool(left), Self::Bool(right)) if left == right => {}
                (Self::Number(left), Self::Number(right)) if left == right => {}
                (Self::String(left), Self::String(right))
                | (Self::Name(left), Self::Name(right))
                    if left == right => {}
                (Self::Array(left), Self::Array(right)) if left.len() == right.len() => {
                    pending.extend(left.iter().zip(right));
                }
                (Self::Dict(left), Self::Dict(right)) if left.len() == right.len() => {
                    for ((left_key, left), (right_key, right)) in left.iter().zip(right) {
                        if left_key != right_key {
                            return false;
                        }
                        pending.push((left, right));
                    }
                }
                _ => return false,
            }
        }
        true
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        enum Frame {
            Array(Vec<Value>),
            Dict(BTreeMap<String, Value>),
        }

        fn take_frame(value: &mut Value) -> Option<Frame> {
            match value {
                Value::Array(values) => Some(Frame::Array(std::mem::take(values))),
                Value::Dict(values) => Some(Frame::Dict(std::mem::take(values))),
                _ => None,
            }
        }

        let Some(frame) = take_frame(self) else {
            return;
        };
        let mut stack = vec![frame];
        while let Some(frame) = stack.last_mut() {
            let child = match frame {
                Frame::Array(values) => values.pop(),
                Frame::Dict(values) => values.pop_last().map(|(_, value)| value),
            };
            if let Some(mut child) = child {
                if let Some(frame) = take_frame(&mut child) {
                    stack.push(frame);
                }
            } else {
                stack.pop();
            }
        }
    }
}

impl Value {
    pub(crate) fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Dict(values) => values.get(key),
            _ => None,
        }
    }

    pub(crate) fn index(&self, index: usize) -> Option<&Self> {
        match self {
            Self::Array(values) => values.get(index),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) | Self::Name(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_i64(&self) -> Option<i64> {
        let value = self.as_f64()?;
        (value.fract() == 0.0 && value >= i64::MIN as f64 && value < -(i64::MIN as f64))
            .then_some(value as i64)
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Error {
    #[error("malformed COS data: {0}")]
    Malformed(&'static str),
    #[error("unsupported COS string encoding")]
    Encoding,
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Value, Error> {
    enum Frame {
        Array(Vec<Value>),
        Dict {
            values: BTreeMap<String, Value>,
            key: Option<String>,
            terminated: bool,
        },
    }

    fn attach(value: Value, stack: &mut [Frame]) -> Result<Option<Value>, Error> {
        let Some(frame) = stack.last_mut() else {
            return Ok(Some(value));
        };
        match frame {
            Frame::Array(values) => values.push(value),
            Frame::Dict { values, key, .. } => {
                let key = key
                    .take()
                    .expect("dictionary requests a value only after parsing its key");
                if values.insert(key, value).is_some() {
                    return Err(Error::Malformed("duplicate dictionary key"));
                }
            }
        }
        Ok(None)
    }

    let mut parser = Parser { bytes, offset: 0 };
    parser.skip_trivia()?;
    let mut stack = Vec::new();
    if parser.peek() == Some(b'/') {
        stack.push(Frame::Dict {
            values: BTreeMap::new(),
            key: None,
            terminated: false,
        });
    } else {
        match parser.value()? {
            ParsedValue::Complete(value) => {
                parser.skip_trivia()?;
                if parser.offset != bytes.len() {
                    return Err(Error::Malformed("trailing tokens"));
                }
                return Ok(value);
            }
            ParsedValue::Array => stack.push(Frame::Array(Vec::new())),
            ParsedValue::Dict => stack.push(Frame::Dict {
                values: BTreeMap::new(),
                key: None,
                terminated: true,
            }),
        }
    }

    loop {
        parser.skip_trivia()?;
        let close = match stack.last_mut().expect("root parse frame remains present") {
            Frame::Array(_) if parser.peek() == Some(b']') => {
                parser.offset += 1;
                true
            }
            Frame::Array(_) if parser.offset == bytes.len() => {
                return Err(Error::Malformed("unterminated array"));
            }
            Frame::Array(_) => false,
            Frame::Dict { key: Some(_), .. } => false,
            Frame::Dict {
                terminated: true, ..
            } if parser.starts_with(b">>") => {
                parser.offset += 2;
                true
            }
            Frame::Dict {
                terminated: true, ..
            } if parser.offset == bytes.len() => {
                return Err(Error::Malformed("unterminated dictionary"));
            }
            Frame::Dict {
                values,
                terminated: false,
                ..
            } if parser.offset == bytes.len() => {
                let values = std::mem::take(values);
                stack.pop();
                return Ok(Value::Dict(values));
            }
            Frame::Dict { key, .. } => {
                if parser.peek() != Some(b'/') {
                    return Err(Error::Malformed("dictionary key is not a name"));
                }
                *key = Some(parser.name()?);
                continue;
            }
        };

        if close {
            let value = match stack.pop().expect("completed parse frame exists") {
                Frame::Array(values) => Value::Array(values),
                Frame::Dict { values, .. } => Value::Dict(values),
            };
            if let Some(value) = attach(value, &mut stack)? {
                parser.skip_trivia()?;
                if parser.offset != bytes.len() {
                    return Err(Error::Malformed("trailing tokens"));
                }
                return Ok(value);
            }
            continue;
        }

        match parser.value()? {
            ParsedValue::Complete(value) => {
                if let Some(value) = attach(value, &mut stack)? {
                    return Ok(value);
                }
            }
            ParsedValue::Array => stack.push(Frame::Array(Vec::new())),
            ParsedValue::Dict => stack.push(Frame::Dict {
                values: BTreeMap::new(),
                key: None,
                terminated: true,
            }),
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    offset: usize,
}

enum ParsedValue {
    Complete(Value),
    Array,
    Dict,
}

impl Parser<'_> {
    fn value(&mut self) -> Result<ParsedValue, Error> {
        self.skip_trivia()?;
        if self.starts_with(b"<<") {
            self.offset += 2;
            return Ok(ParsedValue::Dict);
        }
        let value = match self.peek().ok_or(Error::Malformed("missing value"))? {
            b'[' => {
                self.offset += 1;
                return Ok(ParsedValue::Array);
            }
            b'(' => self.literal_string(),
            b'<' => self.hex_string(),
            b'/' => self.name().map(Value::Name),
            b't' if self.consume_keyword(b"true") => Ok(Value::Bool(true)),
            b'f' if self.consume_keyword(b"false") => Ok(Value::Bool(false)),
            b'n' if self.consume_keyword(b"null") => Ok(Value::Null),
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.number(),
            _ => Err(Error::Malformed("unknown token")),
        }?;
        Ok(ParsedValue::Complete(value))
    }

    fn name(&mut self) -> Result<String, Error> {
        if self.peek() != Some(b'/') {
            return Err(Error::Malformed("missing name"));
        }
        self.offset += 1;
        let mut bytes = Vec::new();
        while let Some(byte) = self.peek() {
            if is_delimiter(byte) || byte.is_ascii_whitespace() {
                break;
            }
            self.offset += 1;
            if byte == b'#' {
                let hi = self
                    .take()
                    .and_then(hex)
                    .ok_or(Error::Malformed("name escape"))?;
                let lo = self
                    .take()
                    .and_then(hex)
                    .ok_or(Error::Malformed("name escape"))?;
                bytes.push((hi << 4) | lo);
            } else {
                bytes.push(byte);
            }
        }
        String::from_utf8(bytes).map_err(|_| Error::Encoding)
    }

    fn literal_string(&mut self) -> Result<Value, Error> {
        self.offset += 1;
        let mut bytes = Vec::new();
        let mut nesting = 1usize;
        while let Some(byte) = self.take() {
            match byte {
                b'(' => {
                    nesting += 1;
                    bytes.push(byte);
                }
                b')' => {
                    nesting -= 1;
                    if nesting == 0 {
                        return decode_string(&bytes).map(Value::String);
                    }
                    bytes.push(byte);
                }
                b'\\' => self.escape(&mut bytes)?,
                _ => bytes.push(byte),
            }
        }
        Err(Error::Malformed("unterminated string"))
    }

    fn escape(&mut self, output: &mut Vec<u8>) -> Result<(), Error> {
        let byte = self.take().ok_or(Error::Malformed("unterminated escape"))?;
        match byte {
            b'n' => output.push(b'\n'),
            b'r' => output.push(b'\r'),
            b't' => output.push(b'\t'),
            b'b' => output.push(8),
            b'f' => output.push(12),
            b'(' | b')' | b'\\' => output.push(byte),
            b'\r' => {
                if self.peek() == Some(b'\n') {
                    self.offset += 1;
                }
            }
            b'\n' => {}
            b'0'..=b'7' => {
                let mut value = byte - b'0';
                for _ in 0..2 {
                    let Some(next @ b'0'..=b'7') = self.peek() else {
                        break;
                    };
                    self.offset += 1;
                    value = value.wrapping_mul(8).wrapping_add(next - b'0');
                }
                output.push(value);
            }
            _ => output.push(byte),
        }
        Ok(())
    }

    fn hex_string(&mut self) -> Result<Value, Error> {
        self.offset += 1;
        let mut bytes = Vec::new();
        let mut high_nibble = None;
        loop {
            let byte = self
                .take()
                .ok_or(Error::Malformed("unterminated hex string"))?;
            if byte == b'>' {
                break;
            }
            if byte.is_ascii_whitespace() {
                continue;
            }
            let nibble = hex(byte).ok_or(Error::Malformed("hex string digit"))?;
            if let Some(high) = high_nibble.take() {
                bytes.push((high << 4) | nibble);
            } else {
                high_nibble = Some(nibble);
            }
        }
        if let Some(high) = high_nibble {
            bytes.push(high << 4);
        }
        decode_string(&bytes).map(Value::String)
    }

    fn number(&mut self) -> Result<Value, Error> {
        let start = self.offset;
        while let Some(byte) = self.peek() {
            if matches!(byte, b'+' | b'-' | b'.' | b'0'..=b'9') {
                self.offset += 1;
            } else {
                break;
            }
        }
        let token = std::str::from_utf8(&self.bytes[start..self.offset])
            .map_err(|_| Error::Malformed("number encoding"))?;
        let unsigned = token
            .strip_prefix('+')
            .or_else(|| token.strip_prefix('-'))
            .unwrap_or(token);
        if !unsigned.is_empty() && unsigned.bytes().all(|byte| byte.is_ascii_digit()) {
            token
                .parse::<i64>()
                .map_err(|_| Error::Malformed("integer out of range"))?;
        }
        let value = token
            .parse::<f64>()
            .map_err(|_| Error::Malformed("number"))?;
        if !value.is_finite() {
            return Err(Error::Malformed("non-finite number"));
        }
        Ok(Value::Number(value))
    }

    fn skip_trivia(&mut self) -> Result<(), Error> {
        loop {
            while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                self.offset += 1;
            }
            if self.peek() != Some(b'%') {
                return Ok(());
            }
            while let Some(byte) = self.take() {
                if matches!(byte, b'\r' | b'\n') {
                    break;
                }
            }
        }
    }

    fn consume_keyword(&mut self, keyword: &[u8]) -> bool {
        if !self.starts_with(keyword) {
            return false;
        }
        let end = self.offset + keyword.len();
        if self
            .bytes
            .get(end)
            .is_some_and(|byte| !is_delimiter(*byte) && !byte.is_ascii_whitespace())
        {
            return false;
        }
        self.offset = end;
        true
    }

    fn starts_with(&self, bytes: &[u8]) -> bool {
        self.bytes[self.offset..].starts_with(bytes)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn take(&mut self) -> Option<u8> {
        let value = self.peek()?;
        self.offset += 1;
        Some(value)
    }
}

fn decode_string(bytes: &[u8]) -> Result<String, Error> {
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 {
            return Err(Error::Encoding);
        }
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]));
        return String::from_utf16(&units.collect::<Vec<_>>()).map_err(|_| Error::Encoding);
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        if bytes.len() % 2 != 0 {
            return Err(Error::Encoding);
        }
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
        return String::from_utf16(&units.collect::<Vec<_>>()).map_err(|_| Error::Encoding);
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| Error::Encoding)
}

fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_cos_clone_and_equality_preserve_value_semantics() {
        let value =
            parse(br#"[ null true false 2 (string) /name [ ] << /a 1 /b [ 3 ] >> ]"#).unwrap();
        let cloned = value.clone();
        assert!(value == cloned);
        let different =
            parse(br#"[ null true false 2 (string) /name [ ] << /a 1 /c [ 3 ] >> ]"#).unwrap();
        assert!(value != different);
        assert!(Value::String("same".into()) != Value::Name("same".into()));
        assert!(Value::Number(f64::NAN) != Value::Number(f64::NAN));
        assert!(Value::Number(-0.0) == Value::Number(0.0));
        assert!(parse(b"[ 1 2 ]").unwrap() != parse(b"[ 2 1 ]").unwrap());
        assert!(parse(b"[ 1 ]").unwrap() != parse(b"[ 1 2 ]").unwrap());
    }

    #[test]
    fn cos_scale_retains_large_strings_and_flat_arrays() {
        let input = format!("({}z)", "x".repeat(8 * 1024 * 1024));
        let value = parse(input.as_bytes()).unwrap();
        let text = value.as_str().unwrap();
        assert_eq!(text.len(), 8 * 1024 * 1024 + 1);
        assert!(text.ends_with('z'));

        let input = format!("[{}7]", "0 ".repeat(100_000));
        let value = parse(input.as_bytes()).unwrap();
        let values = value.as_array().unwrap();
        assert_eq!(values.len(), 100_001);
        assert_eq!(values.last().and_then(Value::as_i64), Some(7));
        assert!(matches!(
            parse(&input.as_bytes()[..input.len() - 1]),
            Err(Error::Malformed("unterminated array"))
        ));
    }

    #[test]
    fn cos_scale_literal_nesting_is_iterative() {
        let input = format!("{}x{}", "(".repeat(100), ")".repeat(100));
        let value = parse(input.as_bytes()).unwrap();
        assert_eq!(value.as_str().unwrap().len(), 199);
    }

    #[test]
    fn parses_root_dictionary_and_utf16_string() {
        let value = parse(b" /0 << /1 [ 2 true (\\376\\377\\000H\\000i) ] >> ").unwrap();
        let array = value
            .get("0")
            .unwrap()
            .get("1")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(array[0].as_i64(), Some(2));
        assert_eq!(array[1].as_bool(), Some(true));
        assert_eq!(array[2].as_str(), Some("Hi"));
    }

    #[test]
    fn integer_literal_bounds_are_checked_before_f64_rounding() {
        let value = parse(b"/minimum -9223372036854775808 /maximum 9223372036854775807")
            .expect("in-range integer boundaries parse");
        assert_eq!(value.get("minimum").and_then(Value::as_i64), Some(i64::MIN));
        assert!(value.get("maximum").and_then(Value::as_f64).is_some());
        for input in [
            b"/value -9223372036854775809".as_slice(),
            b"/value 9223372036854775808".as_slice(),
        ] {
            assert_eq!(parse(input), Err(Error::Malformed("integer out of range")));
        }
    }

    #[test]
    fn rejects_duplicate_keys() {
        assert_eq!(
            parse(b"/0 1 /0 2"),
            Err(Error::Malformed("duplicate dictionary key"))
        );
    }

    #[test]
    fn deeply_nested_containers_parse_and_drop_on_a_small_stack() {
        const DEPTH: usize = 10_000;
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                let input = format!("{}0{}", "[".repeat(DEPTH), "]".repeat(DEPTH));
                let value = parse(input.as_bytes()).unwrap();
                let mut nested = &value;
                for _ in 0..DEPTH {
                    let values = nested.as_array().unwrap();
                    assert_eq!(values.len(), 1);
                    nested = &values[0];
                }
                assert_eq!(nested.as_i64(), Some(0));

                assert_eq!(
                    parse(&input.as_bytes()[..input.len() - 1]),
                    Err(Error::Malformed("unterminated array"))
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
