//! ISO 10303-21 exchange-structure parser.

use std::collections::HashMap;

use crate::{invalid, KernelResult};

#[derive(Debug, Clone)]
pub(super) enum Value {
    Number(f64),
    Text(String),
    Enumeration(String),
    Reference(usize),
    List(Vec<Value>),
    Typed(Vec<Value>),
    Unset,
    Derived,
}

/// One partial entity: a simple instance has one, a complex instance several.
#[derive(Debug, Clone)]
pub(super) struct Record {
    pub name: String,
    pub values: Vec<Value>,
}

#[derive(Debug, Default)]
pub(super) struct Model {
    pub instances: HashMap<usize, Vec<Record>>,
}

pub(super) fn parse(text: &str) -> KernelResult<Model> {
    let mut parser = Parser { bytes: text.as_bytes(), position: 0 };
    let mut model = Model::default();
    let mut in_data = false;
    loop {
        parser.skip_space();
        match parser.peek() {
            None => break,
            Some(b'#') if in_data => {
                let (id, records) = parser.instance()?;
                model.instances.insert(id, records);
            }
            Some(_) => {
                let keyword = parser.keyword()?;
                parser.skip_space();
                if parser.peek() == Some(b'(') {
                    parser.list()?;
                    parser.skip_space();
                }
                parser.expect(b';')?;
                match keyword.as_str() {
                    "DATA" => in_data = true,
                    "ENDSEC" => in_data = false,
                    "END-ISO-10303-21" => break,
                    _ => {}
                }
            }
        }
    }
    if model.instances.is_empty() {
        return Err(invalid("STEP file contains no entity instances"));
    }
    Ok(model)
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn error(&self, message: &str) -> crate::KernelError {
        let line = self.bytes[..self.position.min(self.bytes.len())]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1;
        invalid(format!("STEP syntax error on line {line}: {message}"))
    }

    fn skip_space(&mut self) {
        loop {
            match self.peek() {
                Some(byte) if byte.is_ascii_whitespace() => self.position += 1,
                Some(b'/') if self.bytes.get(self.position + 1) == Some(&b'*') => {
                    self.position += 2;
                    while self.position < self.bytes.len()
                        && !self.bytes[self.position..].starts_with(b"*/")
                    {
                        self.position += 1;
                    }
                    self.position += 2;
                }
                _ => return,
            }
        }
    }

    fn expect(&mut self, byte: u8) -> KernelResult<()> {
        self.skip_space();
        if self.peek() != Some(byte) {
            return Err(self.error(&format!("expected '{}'", byte as char)));
        }
        self.position += 1;
        Ok(())
    }

    fn keyword(&mut self) -> KernelResult<String> {
        let start = self.position;
        while matches!(self.peek(), Some(byte) if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
            self.position += 1;
        }
        if start == self.position {
            return Err(self.error("expected a keyword"));
        }
        Ok(String::from_utf8_lossy(&self.bytes[start..self.position]).to_ascii_uppercase())
    }

    fn integer(&mut self) -> KernelResult<usize> {
        let start = self.position;
        while matches!(self.peek(), Some(byte) if byte.is_ascii_digit()) {
            self.position += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.position])
            .ok()
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| self.error("expected an instance number"))
    }

    fn instance(&mut self) -> KernelResult<(usize, Vec<Record>)> {
        self.position += 1;
        let id = self.integer()?;
        self.expect(b'=')?;
        self.skip_space();
        let mut records = Vec::new();
        if self.peek() == Some(b'(') {
            self.position += 1;
            loop {
                self.skip_space();
                if self.peek() == Some(b')') {
                    self.position += 1;
                    break;
                }
                records.push(self.record()?);
            }
        } else {
            records.push(self.record()?);
        }
        self.expect(b';')?;
        Ok((id, records))
    }

    fn record(&mut self) -> KernelResult<Record> {
        let name = self.keyword()?;
        self.skip_space();
        let values = self.list()?;
        Ok(Record { name, values })
    }

    fn list(&mut self) -> KernelResult<Vec<Value>> {
        self.expect(b'(')?;
        let mut values = Vec::new();
        self.skip_space();
        if self.peek() == Some(b')') {
            self.position += 1;
            return Ok(values);
        }
        loop {
            values.push(self.value()?);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b')') => {
                    self.position += 1;
                    return Ok(values);
                }
                _ => return Err(self.error("expected ',' or ')'")),
            }
        }
    }

    fn value(&mut self) -> KernelResult<Value> {
        self.skip_space();
        let Some(byte) = self.peek() else {
            return Err(self.error("unexpected end of file"));
        };
        Ok(match byte {
            b'$' => {
                self.position += 1;
                Value::Unset
            }
            b'*' => {
                self.position += 1;
                Value::Derived
            }
            b'#' => {
                self.position += 1;
                Value::Reference(self.integer()?)
            }
            b'\'' => Value::Text(self.text()?),
            b'"' => {
                self.position += 1;
                while matches!(self.peek(), Some(byte) if byte != b'"') {
                    self.position += 1;
                }
                self.position += 1;
                Value::Unset
            }
            b'.' => {
                self.position += 1;
                let name = self.keyword()?;
                self.expect(b'.')?;
                Value::Enumeration(name)
            }
            b'(' => Value::List(self.list()?),
            b'0'..=b'9' | b'+' | b'-' => Value::Number(self.number()?),
            _ => {
                self.keyword()?;
                self.skip_space();
                Value::Typed(self.list()?)
            }
        })
    }

    fn number(&mut self) -> KernelResult<f64> {
        let start = self.position;
        while matches!(self.peek(), Some(byte) if byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.' | b'e' | b'E')) {
            self.position += 1;
        }
        let text = String::from_utf8_lossy(&self.bytes[start..self.position]).replace(".E", ".0E").replace(".e", ".0e");
        text.parse().map_err(|_| self.error("malformed number"))
    }

    /// Skip a string; doubled apostrophes are escaped ones.
    fn text(&mut self) -> KernelResult<String> {
        self.position += 1;
        let mut raw = Vec::new();
        loop {
            match self.peek() {
                None => return Err(self.error("unterminated string")),
                Some(b'\'') if self.bytes.get(self.position + 1) == Some(&b'\'') => {
                    raw.push(b'\'');
                    self.position += 2;
                }
                Some(b'\'') => {
                    self.position += 1;
                    return Ok(decode(&String::from_utf8_lossy(&raw)));
                }
                Some(byte) => {
                    raw.push(byte);
                    self.position += 1;
                }
            }
        }
    }
}

/// Resolve Part 21 string escapes: `\\`, `\S\` and `\X\` for Latin-1, and
/// `\X2\`/`\X4\` hexadecimal runs ended by `\X0\`. Code page switches `\P?\`
/// select Latin-1 here.
fn decode(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    let hex = |digits: &str| u32::from_str_radix(digits, 16).ok();
    while let Some(index) = rest.find('\\') {
        result += &rest[..index];
        rest = &rest[index..];
        if let Some(after) = rest.strip_prefix("\\\\") {
            result.push('\\');
            rest = after;
        } else if let Some(after) = rest.strip_prefix("\\S\\") {
            let mut chars = after.chars();
            if let Some(character) = chars.next() {
                result.extend(char::from_u32(character as u32 + 128));
            }
            rest = chars.as_str();
        } else if let Some((digits, after)) = rest.strip_prefix("\\X\\").and_then(|after| Some((after.get(..2)?, &after[2..]))) {
            result.extend(hex(digits).and_then(char::from_u32));
            rest = after;
        } else if let Some((width, after)) = [(4, "\\X2\\"), (8, "\\X4\\")]
            .into_iter()
            .find_map(|(width, prefix)| Some((width, rest.strip_prefix(prefix)?)))
        {
            let end = after.find("\\X0\\").unwrap_or(after.len());
            let units = after[..end]
                .as_bytes()
                .chunks(width)
                .filter_map(|chunk| hex(std::str::from_utf8(chunk).ok()?))
                .collect::<Vec<_>>();
            if width == 4 {
                let units = units.iter().map(|unit| *unit as u16).collect::<Vec<_>>();
                result += &String::from_utf16_lossy(&units);
            } else {
                result.extend(units.into_iter().filter_map(char::from_u32));
            }
            rest = after.get(end + 4..).unwrap_or("");
        } else if rest.starts_with("\\P") && rest.get(3..4) == Some("\\") {
            rest = &rest[4..];
        } else {
            result.push('\\');
            rest = &rest[1..];
        }
    }
    result + rest
}
