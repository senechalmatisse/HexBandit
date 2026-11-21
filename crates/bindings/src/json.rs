//! JSON minimal (remplace serde_json, indisponible hors ligne).
//!
//! Les nombres sont conservés sous forme textuelle : un entier sur 64 bits
//! (graine) est relu exactement, sans passer par un `f64`.

use std::fmt::{self, Write as _};

/// Valeur JSON.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// `null`.
    Null,
    /// Booléen.
    Bool(bool),
    /// Nombre (texte validé).
    Number(String),
    /// Chaîne.
    String(String),
    /// Tableau.
    Array(Vec<Json>),
    /// Objet (ordre des clés conservé).
    Object(Vec<(String, Json)>),
}

/// Erreur de lecture JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    /// Position (octet) de l'erreur.
    pub position: usize,
    /// Explication.
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "JSON invalide à l'octet {} : {}",
            self.position, self.message
        )
    }
}

impl std::error::Error for JsonError {}

const MAX_DEPTH: usize = 128;

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn error<T>(&self, message: &str) -> Result<T, JsonError> {
        Err(JsonError {
            position: self.pos,
            message: message.to_owned(),
        })
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.bytes.get(self.pos) {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn expect_literal(&mut self, literal: &str, value: Json) -> Result<Json, JsonError> {
        if self.bytes[self.pos..].starts_with(literal.as_bytes()) {
            self.pos += literal.len();
            Ok(value)
        } else {
            self.error("littéral inconnu")
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return self.error("imbrication trop profonde");
        }
        self.skip_ws();
        match self.bytes.get(self.pos) {
            None => self.error("fin de texte inattendue"),
            Some(b'n') => self.expect_literal("null", Json::Null),
            Some(b't') => self.expect_literal("true", Json::Bool(true)),
            Some(b'f') => self.expect_literal("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::String),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => self.error("caractère inattendu"),
        }
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        let digits = |p: &mut Self| {
            let s = p.pos;
            while matches!(p.bytes.get(p.pos), Some(b'0'..=b'9')) {
                p.pos += 1;
            }
            p.pos - s
        };
        if self.bytes.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        match self.bytes.get(self.pos) {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                digits(self);
            }
            _ => return self.error("nombre mal formé"),
        }
        if self.bytes.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            if digits(self) == 0 {
                return self.error("décimales manquantes");
            }
        }
        if matches!(self.bytes.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if digits(self) == 0 {
                return self.error("exposant manquant");
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap_or("0");
        Ok(Json::Number(text.to_owned()))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let Some(chunk) = self.bytes.get(self.pos..self.pos + 4) else {
            return self.error("échappement \\u incomplet");
        };
        let text = std::str::from_utf8(chunk).unwrap_or("");
        let value =
            u32::from_str_radix(text, 16).or_else(|_| self.error("échappement \\u invalide"))?;
        self.pos += 4;
        Ok(value)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.pos += 1; // guillemet ouvrant
        let mut out = String::new();
        loop {
            let start = self.pos;
            while let Some(&b) = self.bytes.get(self.pos) {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            out.push_str(
                std::str::from_utf8(&self.bytes[start..self.pos])
                    .or_else(|_| self.error("UTF-8 invalide"))?,
            );
            match self.bytes.get(self.pos) {
                None => return self.error("chaîne non terminée"),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let Some(&esc) = self.bytes.get(self.pos) else {
                        return self.error("échappement incomplet");
                    };
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                if self.bytes.get(self.pos..self.pos + 2) != Some(b"\\u") {
                                    return self.error("demi-codet isolé");
                                }
                                self.pos += 2;
                                let second = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&second) {
                                    return self.error("demi-codet invalide");
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            match char::from_u32(code) {
                                Some(c) => out.push(c),
                                None => return self.error("point de code invalide"),
                            }
                        }
                        _ => return self.error("échappement inconnu"),
                    }
                }
                Some(_) => return self.error("caractère de contrôle dans une chaîne"),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.bytes.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.bytes.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Array(items));
                }
                _ => return self.error("',' ou ']' attendu"),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.pos += 1;
        let mut fields: Vec<(String, Json)> = Vec::new();
        self.skip_ws();
        if self.bytes.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            return Ok(Json::Object(fields));
        }
        loop {
            self.skip_ws();
            if self.bytes.get(self.pos) != Some(&b'"') {
                return self.error("clé attendue");
            }
            let key = self.string()?;
            if fields.iter().any(|(k, _)| *k == key) {
                return self.error("clé dupliquée");
            }
            self.skip_ws();
            if self.bytes.get(self.pos) != Some(&b':') {
                return self.error("':' attendu");
            }
            self.pos += 1;
            let value = self.value(depth + 1)?;
            fields.push((key, value));
            self.skip_ws();
            match self.bytes.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Object(fields));
                }
                _ => return self.error("',' ou '}' attendu"),
            }
        }
    }
}

impl Json {
    /// Lit un texte JSON complet.
    ///
    /// # Errors
    ///
    /// [`JsonError`] si le texte est invalide.
    pub fn parse(text: &str) -> Result<Json, JsonError> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            pos: 0,
        };
        let value = parser.value(0)?;
        parser.skip_ws();
        if parser.pos != parser.bytes.len() {
            return parser.error("texte en trop après la valeur");
        }
        Ok(value)
    }

    /// Objet construit à partir de paires (clé, valeur).
    #[must_use]
    pub fn object<K: Into<String>>(fields: Vec<(K, Json)>) -> Json {
        Json::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// Nombre entier.
    #[must_use]
    pub fn int<T: Into<i128>>(value: T) -> Json {
        Json::Number(value.into().to_string())
    }

    /// Nombre réel (`null` s'il n'est pas fini).
    #[must_use]
    pub fn float(value: f64) -> Json {
        if value.is_finite() {
            Json::Number(format!("{value}"))
        } else {
            Json::Null
        }
    }

    /// Nombre réel simple précision, écrit sous sa forme la plus courte.
    #[must_use]
    pub fn float32(value: f32) -> Json {
        if value.is_finite() {
            Json::Number(format!("{value}"))
        } else {
            Json::Null
        }
    }

    /// Chaîne.
    #[must_use]
    pub fn str(value: impl Into<String>) -> Json {
        Json::String(value.into())
    }

    /// Valeur du champ `key` d'un objet.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Nom du type JSON (pour les messages d'erreur).
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Json::Null => "null",
            Json::Bool(_) => "booléen",
            Json::Number(_) => "nombre",
            Json::String(_) => "chaîne",
            Json::Array(_) => "tableau",
            Json::Object(_) => "objet",
        }
    }

    fn write_to(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Number(n) => out.push_str(n),
            Json::String(s) => write_string(out, s),
            Json::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write_to(out);
                }
                out.push(']');
            }
            Json::Object(fields) => {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(out, k);
                    out.push(':');
                    v.write_to(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        self.write_to(&mut out);
        f.write_str(&out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let text =
            r#"{"a":[1,-2.5,3e10,true,false,null],"b":"x\"y\\z\n\u00e9\ud83d\ude00","c":{}}"#;
        let v = Json::parse(text).unwrap();
        assert_eq!(v.get("b"), Some(&Json::str("x\"y\\z\né😀")));
        let again = Json::parse(&v.to_string()).unwrap();
        assert_eq!(again, v);
    }

    #[test]
    fn big_integers_are_exact() {
        let v = Json::parse("18446744073709551615").unwrap();
        assert_eq!(v, Json::Number("18446744073709551615".into()));
    }

    #[test]
    fn rejects_invalid_documents() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":1,}",
            "01",
            "1.",
            "\"\\x\"",
            "{\"a\":1,\"a\":2}",
            "nul",
            "[1] 2",
            "\"\u{1}\"",
        ] {
            assert!(Json::parse(bad).is_err(), "{bad:?}");
        }
        let deep = "[".repeat(500);
        assert!(Json::parse(&deep).is_err());
    }

    #[test]
    fn non_finite_floats_are_null() {
        assert_eq!(Json::float(f64::NAN), Json::Null);
        assert_eq!(Json::float32(f32::INFINITY), Json::Null);
        assert_eq!(Json::float32(0.5).to_string(), "0.5");
    }
}
