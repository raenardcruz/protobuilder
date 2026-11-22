use crate::ast::*;
use crate::wkt::lookup_wkt_proto;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Ident(String),
    StringLit(String),
    IntLit(i64),
    FloatLit(String),
    Symbol(char),
    Comment(String),
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    line: usize,
    col: usize,
}

struct Lexer<'a> {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    _src: &'a str,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            chars: src.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            _src: src,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
                continue;
            }
            let line = self.line;
            let col = self.col;

            // Comments
            if ch == '/' && self.peek_next() == Some('/') {
                self.advance();
                self.advance();
                if self.peek() == Some('/') {
                    self.advance();
                }
                let mut text = String::new();
                while let Some(c) = self.peek() {
                    if c == '\n' {
                        break;
                    }
                    text.push(c);
                    self.advance();
                }
                tokens.push(Token {
                    kind: TokenKind::Comment(text.trim().to_string()),
                    line,
                    col,
                });
                continue;
            }
            if ch == '/' && self.peek_next() == Some('*') {
                self.advance();
                self.advance();
                let mut text = String::new();
                while let Some(c) = self.peek() {
                    if c == '*' && self.peek_next() == Some('/') {
                        self.advance();
                        self.advance();
                        break;
                    }
                    text.push(c);
                    self.advance();
                }
                for l in text.lines() {
                    let clean = l.trim().trim_start_matches('*').trim();
                    if !clean.is_empty() {
                        tokens.push(Token {
                            kind: TokenKind::Comment(clean.to_string()),
                            line,
                            col,
                        });
                    }
                }
                continue;
            }

            // String literals
            if ch == '"' || ch == '\'' {
                let quote = ch;
                self.advance();
                let mut s = String::new();
                while let Some(c) = self.peek() {
                    if c == quote {
                        self.advance();
                        break;
                    }
                    if c == '\\' {
                        self.advance();
                        if let Some(esc) = self.advance() {
                            match esc {
                                'n' => s.push('\n'),
                                'r' => s.push('\r'),
                                't' => s.push('\t'),
                                '\\' => s.push('\\'),
                                '"' => s.push('"'),
                                '\'' => s.push('\''),
                                other => s.push(other),
                            }
                        }
                    } else {
                        s.push(c);
                        self.advance();
                    }
                }
                tokens.push(Token {
                    kind: TokenKind::StringLit(s),
                    line,
                    col,
                });
                continue;
            }

            // Numbers (including negative numbers)
            if ch.is_ascii_digit()
                || (ch == '-' && self.peek_next().map_or(false, |n| n.is_ascii_digit()))
            {
                let mut num = String::new();
                num.push(ch);
                self.advance();
                let mut is_float = false;
                let mut is_hex = false;
                if num == "0" && (self.peek() == Some('x') || self.peek() == Some('X')) {
                    is_hex = true;
                    num.push(self.advance().unwrap());
                }
                while let Some(c) = self.peek() {
                    if c.is_ascii_hexdigit() && is_hex {
                        num.push(c);
                        self.advance();
                    } else if c.is_ascii_digit() {
                        num.push(c);
                        self.advance();
                    } else if !is_hex && (c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-')
                    {
                        is_float = true;
                        num.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
                if is_float {
                    tokens.push(Token {
                        kind: TokenKind::FloatLit(num),
                        line,
                        col,
                    });
                } else {
                    let parsed = if is_hex {
                        i64::from_str_radix(num.trim_start_matches("0x").trim_start_matches("0X"), 16)
                    } else {
                        num.parse::<i64>()
                    }
                    .map_err(|e| format!("Invalid integer literal '{}' at {}:{}: {}", num, line, col, e))?;
                    tokens.push(Token {
                        kind: TokenKind::IntLit(parsed),
                        line,
                        col,
                    });
                }
                continue;
            }

            // Identifiers (may include leading dot or internal dots when resolving qualified names)
            if ch.is_ascii_alphabetic() || ch == '_' {
                let mut ident = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
                        ident.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
                tokens.push(Token {
                    kind: TokenKind::Ident(ident),
                    line,
                    col,
                });
                continue;
            }

            // Leading dot for fully-qualified type `.pkg.Type`
            if ch == '.'
                && self
                    .peek_next()
                    .map_or(false, |n| n.is_ascii_alphabetic() || n == '_')
            {
                let mut ident = String::from(".");
                self.advance();
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
                        ident.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
                tokens.push(Token {
                    kind: TokenKind::Ident(ident),
                    line,
                    col,
                });
                continue;
            }

            // Symbols: ; { } ( ) [ ] < > = , :
            self.advance();
            tokens.push(Token {
                kind: TokenKind::Symbol(ch),
                line,
                col,
            });
        }
        Ok(tokens)
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    pending_comments: Vec<String>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            pending_comments: Vec::new(),
        }
    }

    fn drain_comments(&mut self) {
        while let Some(tok) = self.tokens.get(self.pos) {
            if let TokenKind::Comment(c) = &tok.kind {
                if !c.is_empty() {
                    self.pending_comments.push(c.clone());
                }
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn take_comments(&mut self) -> Vec<String> {
        self.drain_comments();
        std::mem::take(&mut self.pending_comments)
    }

    fn peek(&mut self) -> Option<&Token> {
        self.drain_comments();
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        self.drain_comments();
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn expect_symbol(&mut self, sym: char) -> Result<(), String> {
        match self.next() {
            Some(Token {
                kind: TokenKind::Symbol(c),
                ..
            }) if c == sym => Ok(()),
            Some(tok) => Err(format!(
                "Expected symbol '{}' at {}:{}, found {:?}",
                sym, tok.line, tok.col, tok.kind
            )),
            None => Err(format!("Unexpected EOF while expecting '{}'", sym)),
        }
    }

    fn eat_symbol(&mut self, sym: char) -> bool {
        if let Some(Token {
            kind: TokenKind::Symbol(c),
            ..
        }) = self.peek()
        {
            if *c == sym {
                self.next();
                return true;
            }
        }
        false
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        match self.next() {
            Some(Token {
                kind: TokenKind::Ident(s),
                ..
            }) => Ok(s),
            Some(tok) => Err(format!(
                "Expected identifier at {}:{}, found {:?}",
                tok.line, tok.col, tok.kind
            )),
            None => Err("Unexpected EOF while expecting identifier".to_string()),
        }
    }

    fn expect_string(&mut self) -> Result<String, String> {
        match self.next() {
            Some(Token {
                kind: TokenKind::StringLit(mut s),
                ..
            }) => {
                // Support adjacent string literal concatenation in proto files
                while let Some(Token {
                    kind: TokenKind::StringLit(_),
                    ..
                }) = self.peek()
                {
                    if let Some(Token {
                        kind: TokenKind::StringLit(next_s),
                        ..
                    }) = self.next()
                    {
                        s.push_str(&next_s);
                    }
                }
                Ok(s)
            }
            Some(tok) => Err(format!(
                "Expected string literal at {}:{}, found {:?}",
                tok.line, tok.col, tok.kind
            )),
            None => Err("Unexpected EOF while expecting string literal".to_string()),
        }
    }

    fn expect_int(&mut self) -> Result<i32, String> {
        match self.next() {
            Some(Token {
                kind: TokenKind::IntLit(v),
                ..
            }) => Ok(v as i32),
            Some(tok) => Err(format!(
                "Expected integer at {}:{}, found {:?}",
                tok.line, tok.col, tok.kind
            )),
            None => Err("Unexpected EOF while expecting integer".to_string()),
        }
    }

    fn parse_file(&mut self, name: String, source_path: PathBuf, is_wkt: bool) -> Result<ProtoFile, String> {
        let mut file = ProtoFile {
            name,
            source_path,
            is_wkt,
            syntax: Syntax::Proto3,
            ..Default::default()
        };

        while self.peek().is_some() {
            if self.eat_symbol(';') {
                continue;
            }
            let comments = self.take_comments();
            let kw = self.expect_ident()?;
            match kw.as_str() {
                "syntax" => {
                    if file.comments.is_empty() && !comments.is_empty() {
                        file.comments = comments;
                    }
                    self.expect_symbol('=')?;
                    let val = self.expect_string()?;
                    self.expect_symbol(';')?;
                    file.syntax = match val.as_str() {
                        "proto2" => Syntax::Proto2,
                        "proto3" => Syntax::Proto3,
                        other => Syntax::Edition(other.to_string()),
                    };
                }
                "edition" => {
                    if file.comments.is_empty() && !comments.is_empty() {
                        file.comments = comments;
                    }
                    self.expect_symbol('=')?;
                    let val = self.expect_string()?;
                    self.expect_symbol(';')?;
                    file.syntax = Syntax::Edition(val);
                }
                "package" => {
                    file.package = self.expect_ident()?;
                    self.expect_symbol(';')?;
                }
                "import" => {
                    let mut kind = ImportKind::Normal;
                    if let Some(Token {
                        kind: TokenKind::Ident(modifier),
                        ..
                    }) = self.peek()
                    {
                        match modifier.as_str() {
                            "public" => {
                                kind = ImportKind::Public;
                                self.next();
                            }
                            "weak" => {
                                kind = ImportKind::Weak;
                                self.next();
                            }
                            "option" => {
                                kind = ImportKind::Weak;
                                self.next();
                            }
                            _ => {}
                        }
                    }
                    let path = self.expect_string()?;
                    self.expect_symbol(';')?;
                    file.imports.push(Import { path, kind });
                }
                "option" => {
                    let opt = self.parse_option_decl()?;
                    file.options.push(opt);
                }
                "message" => {
                    let prefix = if file.package.is_empty() {
                        String::new()
                    } else {
                        file.package.clone()
                    };
                    let msg = self.parse_message(&prefix, comments)?;
                    file.messages.push(msg);
                }
                "enum" => {
                    let prefix = if file.package.is_empty() {
                        String::new()
                    } else {
                        file.package.clone()
                    };
                    let en = self.parse_enum(&prefix, comments)?;
                    file.enums.push(en);
                }
                "service" => {
                    let prefix = if file.package.is_empty() {
                        String::new()
                    } else {
                        file.package.clone()
                    };
                    let svc = self.parse_service(&prefix, comments)?;
                    file.services.push(svc);
                }
                "extend" => {
                    // Consume extend block
                    let _target = self.expect_ident()?;
                    self.skip_balanced_braces()?;
                }
                other => {
                    return Err(format!("Unexpected top-level keyword '{}'", other));
                }
            }
        }
        Ok(file)
    }

    fn parse_option_name(&mut self) -> Result<String, String> {
        let mut name = String::new();
        if self.eat_symbol('(') {
            name.push('(');
            let inner = self.expect_ident()?;
            name.push_str(&inner);
            self.expect_symbol(')')?;
            name.push(')');
            while let Some(Token {
                kind: TokenKind::Ident(suffix),
                ..
            }) = self.peek()
            {
                if suffix.starts_with('.') {
                    name.push_str(&self.expect_ident()?);
                } else {
                    break;
                }
            }
            if self.eat_symbol('.') {
                name.push('.');
                name.push_str(&self.expect_ident()?);
            }
        } else {
            name = self.expect_ident()?;
        }
        Ok(name)
    }

    fn parse_option_value(&mut self) -> Result<String, String> {
        if self.eat_symbol('{') {
            // Aggregate option value { key: "val" ... }
            let mut depth = 1;
            let mut out = String::from("{");
            while let Some(tok) = self.next() {
                match tok.kind {
                    TokenKind::Symbol('{') => {
                        depth += 1;
                        out.push('{');
                    }
                    TokenKind::Symbol('}') => {
                        depth -= 1;
                        out.push('}');
                        if depth == 0 {
                            break;
                        }
                    }
                    TokenKind::Symbol(c) => {
                        out.push(c);
                        out.push(' ');
                    }
                    TokenKind::Ident(s) | TokenKind::FloatLit(s) => {
                        out.push_str(&s);
                        out.push(' ');
                    }
                    TokenKind::StringLit(s) => {
                        out.push('"');
                        out.push_str(&s);
                        out.push('"');
                    }
                    TokenKind::IntLit(i) => {
                        out.push_str(&i.to_string());
                    }
                    TokenKind::Comment(_) => {}
                }
            }
            return Ok(out);
        }
        match self.next() {
            Some(Token {
                kind: TokenKind::StringLit(s),
                ..
            }) => Ok(s),
            Some(Token {
                kind: TokenKind::IntLit(i),
                ..
            }) => Ok(i.to_string()),
            Some(Token {
                kind: TokenKind::FloatLit(f),
                ..
            }) => Ok(f),
            Some(Token {
                kind: TokenKind::Ident(id),
                ..
            }) => Ok(id),
            Some(tok) => Err(format!(
                "Invalid option value at {}:{}: {:?}",
                tok.line, tok.col, tok.kind
            )),
            None => Err("Unexpected EOF in option value".to_string()),
        }
    }

    fn parse_option_decl(&mut self) -> Result<ProtoOption, String> {
        let name = self.parse_option_name()?;
        self.expect_symbol('=')?;
        let value = self.parse_option_value()?;
        self.expect_symbol(';')?;
        Ok(ProtoOption { name, value })
    }

    fn parse_inline_options(&mut self) -> Result<Vec<ProtoOption>, String> {
        let mut opts = Vec::new();
        if !self.eat_symbol('[') {
            return Ok(opts);
        }
        while !self.eat_symbol(']') {
            let name = self.parse_option_name()?;
            self.expect_symbol('=')?;
            let value = self.parse_option_value()?;
            opts.push(ProtoOption { name, value });
            let _ = self.eat_symbol(',');
        }
        Ok(opts)
    }

    fn skip_balanced_braces(&mut self) -> Result<(), String> {
        self.expect_symbol('{')?;
        let mut depth = 1;
        while let Some(tok) = self.next() {
            match tok.kind {
                TokenKind::Symbol('{') => depth += 1,
                TokenKind::Symbol('}') => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        Err("Unclosed '{' block".to_string())
    }

    fn parse_reserved(&mut self) -> Result<(Vec<ReservedRange>, Vec<String>), String> {
        let mut ranges = Vec::new();
        let mut names = Vec::new();
        loop {
            match self.peek().cloned() {
                Some(Token {
                    kind: TokenKind::StringLit(_),
                    ..
                }) => {
                    names.push(self.expect_string()?);
                }
                Some(Token {
                    kind: TokenKind::Ident(id),
                    ..
                }) if id != "to" && id != "max" => {
                    names.push(self.expect_ident()?);
                }
                Some(Token {
                    kind: TokenKind::IntLit(_),
                    ..
                }) => {
                    let start = self.expect_int()?;
                    let mut end = start;
                    if let Some(Token {
                        kind: TokenKind::Ident(kw),
                        ..
                    }) = self.peek()
                    {
                        if kw == "to" {
                            self.next();
                            if let Some(Token {
                                kind: TokenKind::Ident(m),
                                ..
                            }) = self.peek()
                            {
                                if m == "max" {
                                    self.next();
                                    end = 536_870_911;
                                } else {
                                    end = self.expect_int()?;
                                }
                            } else {
                                end = self.expect_int()?;
                            }
                        }
                    }
                    ranges.push(ReservedRange { start, end });
                }
                _ => break,
            }
            if !self.eat_symbol(',') {
                break;
            }
        }
        self.expect_symbol(';')?;
        Ok((ranges, names))
    }

    fn parse_message(&mut self, parent_scope: &str, comments: Vec<String>) -> Result<Message, String> {
        let name = self.expect_ident()?;
        let full_name = if parent_scope.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", parent_scope, name)
        };
        self.expect_symbol('{')?;

        let mut msg = Message {
            name,
            full_name: full_name.clone(),
            fields: Vec::new(),
            oneofs: Vec::new(),
            nested_messages: Vec::new(),
            nested_enums: Vec::new(),
            options: Vec::new(),
            reserved_ranges: Vec::new(),
            reserved_names: Vec::new(),
            comments,
        };

        while !self.eat_symbol('}') {
            if self.eat_symbol(';') {
                continue;
            }
            let item_comments = self.take_comments();
            if self.eat_symbol('}') {
                break;
            }
            let kw = self.expect_ident()?;
            match kw.as_str() {
                "message" => {
                    let nested = self.parse_message(&full_name, item_comments)?;
                    msg.nested_messages.push(nested);
                }
                "enum" => {
                    let nested_en = self.parse_enum(&full_name, item_comments)?;
                    msg.nested_enums.push(nested_en);
                }
                "option" => {
                    let opt = self.parse_option_decl()?;
                    msg.options.push(opt);
                }
                "reserved" => {
                    let (mut r, mut n) = self.parse_reserved()?;
                    msg.reserved_ranges.append(&mut r);
                    msg.reserved_names.append(&mut n);
                }
                "extensions" => {
                    let _ = self.parse_reserved()?;
                }
                "extend" => {
                    let _ = self.expect_ident()?;
                    self.skip_balanced_braces()?;
                }
                "oneof" => {
                    let oneof_name = self.expect_ident()?;
                    self.expect_symbol('{')?;
                    let mut oneof_fields = Vec::new();
                    while !self.eat_symbol('}') {
                        if self.eat_symbol(';') {
                            continue;
                        }
                        let f_comments = self.take_comments();
                        if self.eat_symbol('}') {
                            break;
                        }
                        let ty_ident = self.expect_ident()?;
                        if ty_ident == "option" {
                            let _ = self.parse_option_decl()?;
                            continue;
                        }
                        let f_type = self.parse_type_from_first_token(&ty_ident)?;
                        let f_name = self.expect_ident()?;
                        self.expect_symbol('=')?;
                        let f_num = self.expect_int()?;
                        let f_opts = self.parse_inline_options()?;
                        self.expect_symbol(';')?;
                        oneof_fields.push(Field {
                            name: f_name,
                            number: f_num,
                            label: FieldLabel::Optional,
                            field_type: f_type,
                            options: f_opts,
                            comments: f_comments,
                            oneof_name: Some(oneof_name.clone()),
                        });
                    }
                    msg.oneofs.push(Oneof {
                        name: oneof_name,
                        fields: oneof_fields,
                        comments: item_comments,
                    });
                }
                "map" => {
                    let f_type = self.parse_map_type()?;
                    let f_name = self.expect_ident()?;
                    self.expect_symbol('=')?;
                    let f_num = self.expect_int()?;
                    let f_opts = self.parse_inline_options()?;
                    self.expect_symbol(';')?;
                    msg.fields.push(Field {
                        name: f_name,
                        number: f_num,
                        label: FieldLabel::Singular,
                        field_type: f_type,
                        options: f_opts,
                        comments: item_comments,
                        oneof_name: None,
                    });
                }
                "repeated" | "optional" | "required" | "singular" => {
                    let label = match kw.as_str() {
                        "repeated" => FieldLabel::Repeated,
                        "optional" => FieldLabel::Optional,
                        "required" => FieldLabel::Required,
                        _ => FieldLabel::Singular,
                    };
                    let ty_token = self.expect_ident()?;
                    let f_type = self.parse_type_from_first_token(&ty_token)?;
                    let f_name = self.expect_ident()?;
                    self.expect_symbol('=')?;
                    let f_num = self.expect_int()?;
                    let f_opts = self.parse_inline_options()?;
                    self.expect_symbol(';')?;
                    msg.fields.push(Field {
                        name: f_name,
                        number: f_num,
                        label,
                        field_type: f_type,
                        options: f_opts,
                        comments: item_comments,
                        oneof_name: None,
                    });
                }
                first_type_tok => {
                    let f_type = self.parse_type_from_first_token(first_type_tok)?;
                    let f_name = self.expect_ident()?;
                    self.expect_symbol('=')?;
                    let f_num = self.expect_int()?;
                    let f_opts = self.parse_inline_options()?;
                    self.expect_symbol(';')?;
                    msg.fields.push(Field {
                        name: f_name,
                        number: f_num,
                        label: FieldLabel::Singular,
                        field_type: f_type,
                        options: f_opts,
                        comments: item_comments,
                        oneof_name: None,
                    });
                }
            }
        }
        Ok(msg)
    }

    fn parse_map_type(&mut self) -> Result<FieldType, String> {
        self.expect_symbol('<')?;
        let key_tok = self.expect_ident()?;
        let key_type = self.parse_type_from_first_token(&key_tok)?;
        self.expect_symbol(',')?;
        let val_tok = self.expect_ident()?;
        let val_type = self.parse_type_from_first_token(&val_tok)?;
        self.expect_symbol('>')?;
        Ok(FieldType::Map(Box::new(key_type), Box::new(val_type)))
    }

    fn parse_type_from_first_token(&mut self, token: &str) -> Result<FieldType, String> {
        if token == "map" {
            return self.parse_map_type();
        }
        if let Some(sc) = ScalarType::from_name(token) {
            return Ok(FieldType::Scalar(sc));
        }
        Ok(FieldType::Named {
            raw: token.to_string(),
            resolved_fqn: token.trim_start_matches('.').to_string(),
            kind: ResolvedKind::Unknown,
        })
    }

    fn parse_enum(&mut self, parent_scope: &str, comments: Vec<String>) -> Result<EnumDef, String> {
        let name = self.expect_ident()?;
        let full_name = if parent_scope.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", parent_scope, name)
        };
        self.expect_symbol('{')?;
        let mut en = EnumDef {
            name,
            full_name,
            values: Vec::new(),
            options: Vec::new(),
            reserved_ranges: Vec::new(),
            reserved_names: Vec::new(),
            comments,
        };

        while !self.eat_symbol('}') {
            if self.eat_symbol(';') {
                continue;
            }
            let val_comments = self.take_comments();
            if self.eat_symbol('}') {
                break;
            }
            let ident = self.expect_ident()?;
            match ident.as_str() {
                "option" => {
                    en.options.push(self.parse_option_decl()?);
                }
                "reserved" => {
                    let (mut r, mut n) = self.parse_reserved()?;
                    en.reserved_ranges.append(&mut r);
                    en.reserved_names.append(&mut n);
                }
                val_name => {
                    self.expect_symbol('=')?;
                    let number = self.expect_int()?;
                    let options = self.parse_inline_options()?;
                    self.expect_symbol(';')?;
                    en.values.push(EnumValue {
                        name: val_name.to_string(),
                        number,
                        options,
                        comments: val_comments,
                    });
                }
            }
        }
        Ok(en)
    }

    fn parse_service(&mut self, parent_scope: &str, comments: Vec<String>) -> Result<Service, String> {
        let name = self.expect_ident()?;
        let full_name = if parent_scope.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", parent_scope, name)
        };
        self.expect_symbol('{')?;
        let mut svc = Service {
            name,
            full_name,
            methods: Vec::new(),
            options: Vec::new(),
            comments,
        };

        while !self.eat_symbol('}') {
            if self.eat_symbol(';') {
                continue;
            }
            let m_comments = self.take_comments();
            if self.eat_symbol('}') {
                break;
            }
            let kw = self.expect_ident()?;
            match kw.as_str() {
                "option" => {
                    svc.options.push(self.parse_option_decl()?);
                }
                "rpc" => {
                    let rpc_name = self.expect_ident()?;
                    self.expect_symbol('(')?;
                    let mut client_streaming = false;
                    let first_in = self.expect_ident()?;
                    let input_type = if first_in == "stream" {
                        client_streaming = true;
                        self.expect_ident()?
                    } else {
                        first_in
                    };
                    self.expect_symbol(')')?;
                    let ret_kw = self.expect_ident()?;
                    if ret_kw != "returns" {
                        return Err(format!("Expected 'returns' in rpc '{}', found '{}'", rpc_name, ret_kw));
                    }
                    self.expect_symbol('(')?;
                    let mut server_streaming = false;
                    let first_out = self.expect_ident()?;
                    let output_type = if first_out == "stream" {
                        server_streaming = true;
                        self.expect_ident()?
                    } else {
                        first_out
                    };
                    self.expect_symbol(')')?;

                    let mut rpc_options = Vec::new();
                    if self.eat_symbol('{') {
                        while !self.eat_symbol('}') {
                            if self.eat_symbol(';') {
                                continue;
                            }
                            let _ = self.take_comments();
                            if self.eat_symbol('}') {
                                break;
                            }
                            let opt_kw = self.expect_ident()?;
                            if opt_kw == "option" {
                                rpc_options.push(self.parse_option_decl()?);
                            } else {
                                return Err(format!("Unexpected token '{}' inside rpc block", opt_kw));
                            }
                        }
                        let _ = self.eat_symbol(';');
                    } else {
                        self.expect_symbol(';')?;
                    }

                    svc.methods.push(RpcMethod {
                        name: rpc_name,
                        input_resolved_fqn: input_type.trim_start_matches('.').to_string(),
                        input_type,
                        output_resolved_fqn: output_type.trim_start_matches('.').to_string(),
                        output_type,
                        client_streaming,
                        server_streaming,
                        options: rpc_options,
                        comments: m_comments,
                    });
                }
                other => {
                    return Err(format!("Unexpected keyword '{}' in service '{}'", other, svc.name));
                }
            }
        }
        Ok(svc)
    }
}

pub fn parse_proto_str(content: &str, name: &str, path: PathBuf, is_wkt: bool) -> Result<ProtoFile, String> {
    let mut lexer = Lexer::new(content);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse_file(name.to_string(), path, is_wkt)
}

/// Discovers `.proto` files from file or directory paths.
pub fn discover_proto_files(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();

    fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>, seen: &mut BTreeSet<PathBuf>) -> Result<(), String> {
        let entries = fs::read_dir(dir)
            .map_err(|e| format!("Failed to read directory '{}': {}", dir.display(), e))?;
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|d| d.path())).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with('.') && name != "target" && name != "node_modules" {
                    walk_dir(&p, out, seen)?;
                }
            } else if p.extension().and_then(|e| e.to_str()) == Some("proto") {
                let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
                if seen.insert(canon) {
                    out.push(p);
                }
            }
        }
        Ok(())
    }

    for input in inputs {
        if !input.exists() {
            return Err(format!("Input path '{}' does not exist", input.display()));
        }
        if input.is_dir() {
            walk_dir(input, &mut result, &mut seen)?;
        } else {
            let canon = input.canonicalize().unwrap_or_else(|_| input.clone());
            if seen.insert(canon) {
                result.push(input.clone());
            }
        }
    }

    if result.is_empty() {
        return Err("No .proto files found in the specified input paths".to_string());
    }
    Ok(result)
}

/// Parses a set of `.proto` files along with their transitive imports (using `-I` paths,
/// parent directories, and the embedded Google Well-Known Types virtual filesystem),
/// resolves all message/enum type references, and runs semantic validation checks.
pub fn load_and_resolve_bundle(
    input_files: &[PathBuf],
    include_dirs: &[PathBuf],
) -> Result<SchemaBundle, String> {
    let mut search_dirs: Vec<PathBuf> = include_dirs.to_vec();
    for f in input_files {
        if let Some(parent) = f.parent() {
            let p = if parent.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                parent.to_path_buf()
            };
            if !search_dirs.contains(&p) {
                search_dirs.push(p);
            }
        }
    }
    let cwd = PathBuf::from(".");
    if !search_dirs.contains(&cwd) {
        search_dirs.push(cwd);
    }

    let mut parsed_files: Vec<ProtoFile> = Vec::new();
    let mut visited_names: BTreeSet<String> = BTreeSet::new();
    let mut visited_paths: BTreeSet<PathBuf> = BTreeSet::new();

    fn load_recursive(
        logical_name: &str,
        disk_path: Option<&Path>,
        search_dirs: &[PathBuf],
        parsed_files: &mut Vec<ProtoFile>,
        visited_names: &mut BTreeSet<String>,
        visited_paths: &mut BTreeSet<PathBuf>,
    ) -> Result<(), String> {
        if visited_names.contains(logical_name) {
            return Ok(());
        }
        if let Some(dp) = disk_path {
            let canon = dp.canonicalize().unwrap_or_else(|_| dp.to_path_buf());
            if visited_paths.contains(&canon) {
                return Ok(());
            }
        }

        let (content, actual_path, is_wkt) = if let Some(dp) = disk_path {
            let text = fs::read_to_string(dp)
                .map_err(|e| format!("Failed to read '{}': {}", dp.display(), e))?;
            (text, dp.to_path_buf(), false)
        } else if let Some(wkt_src) = lookup_wkt_proto(logical_name) {
            (wkt_src.to_string(), PathBuf::from(logical_name), true)
        } else {
            let mut found = None;
            for dir in search_dirs {
                let candidate = dir.join(logical_name);
                if candidate.exists() && candidate.is_file() {
                    found = Some(candidate);
                    break;
                }
            }
            if let Some(cand) = found {
                let text = fs::read_to_string(&cand)
                    .map_err(|e| format!("Failed to read import '{}': {}", cand.display(), e))?;
                (text, cand, false)
            } else {
                return Err(format!(
                    "Import '{}' not found in search paths {:?} or built-in Well-Known Types",
                    logical_name, search_dirs
                ));
            }
        };

        visited_names.insert(logical_name.to_string());
        if !is_wkt {
            let canon = actual_path
                .canonicalize()
                .unwrap_or_else(|_| actual_path.clone());
            visited_paths.insert(canon);
        }

        let mut parsed = parse_proto_str(&content, logical_name, actual_path.clone(), is_wkt)?;
        for imp in parsed.imports.clone() {
            load_recursive(
                &imp.path,
                None,
                search_dirs,
                parsed_files,
                visited_names,
                visited_paths,
            )?;
            // If this file imports a built-in Well-Known Type, inline the WKT messages/enums
            // so generated code across all 15 languages compiles standalone with zero external runtime deps!
            if !is_wkt {
                if let Some(wkt_file) = parsed_files.iter().find(|f| f.is_wkt && f.name == imp.path).cloned() {
                    for wm in wkt_file.messages {
                        if !parsed.messages.iter().any(|m| m.name == wm.name) {
                            parsed.messages.insert(0, wm);
                        }
                    }
                    for we in wkt_file.enums {
                        if !parsed.enums.iter().any(|e| e.name == we.name) {
                            parsed.enums.insert(0, we);
                        }
                    }
                }
            }
        }
        parsed_files.push(parsed);
        Ok(())
    }

    for input in input_files {
        let logical = input
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("input.proto")
            .to_string();
        load_recursive(
            &logical,
            Some(input),
            &search_dirs,
            &mut parsed_files,
            &mut visited_names,
            &mut visited_paths,
        )?;
    }

    // Build global type registry
    let mut type_registry: BTreeMap<String, ResolvedKind> = BTreeMap::new();
    fn register_msg(m: &Message, reg: &mut BTreeMap<String, ResolvedKind>) {
        reg.insert(m.full_name.clone(), ResolvedKind::Message);
        reg.insert(m.name.clone(), ResolvedKind::Message);
        for e in &m.nested_enums {
            reg.insert(e.full_name.clone(), ResolvedKind::Enum);
            reg.insert(e.name.clone(), ResolvedKind::Enum);
            reg.insert(format!("{}.{}", m.name, e.name), ResolvedKind::Enum);
        }
        for nm in &m.nested_messages {
            reg.insert(format!("{}.{}", m.name, nm.name), ResolvedKind::Message);
            register_msg(nm, reg);
        }
    }

    for file in &parsed_files {
        for e in &file.enums {
            type_registry.insert(e.full_name.clone(), ResolvedKind::Enum);
            type_registry.insert(e.name.clone(), ResolvedKind::Enum);
        }
        for m in &file.messages {
            register_msg(m, &mut type_registry);
        }
    }

    // Resolve field types and validate
    let mut diagnostics = Vec::new();
    for file in &mut parsed_files {
        let pkg = file.package.clone();
        let file_name = file.name.clone();
        for m in &mut file.messages {
            resolve_and_validate_message(m, &pkg, &file_name, &type_registry, &mut diagnostics);
        }
        for e in &file.enums {
            if e.values.is_empty() {
                diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    file: file_name.clone(),
                    message: format!("Enum '{}' must have at least one value", e.name),
                });
            } else if file.syntax == Syntax::Proto3 && e.values[0].number != 0 {
                diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Warning,
                    file: file_name.clone(),
                    message: format!(
                        "In proto3, the first enum value of '{}' should be 0 (found {})",
                        e.name, e.values[0].number
                    ),
                });
            }
        }
        for s in &mut file.services {
            for rpc in &mut s.methods {
                let in_clean = rpc.input_type.trim_start_matches('.');
                if !type_registry.contains_key(in_clean)
                    && !type_registry.contains_key(&format!("{}.{}", pkg, in_clean))
                {
                    diagnostics.push(Diagnostic {
                        level: DiagnosticLevel::Error,
                        file: file_name.clone(),
                        message: format!(
                            "RPC '{}.{}' references unresolved input message type '{}'",
                            s.name, rpc.name, rpc.input_type
                        ),
                    });
                }
                let out_clean = rpc.output_type.trim_start_matches('.');
                if !type_registry.contains_key(out_clean)
                    && !type_registry.contains_key(&format!("{}.{}", pkg, out_clean))
                {
                    diagnostics.push(Diagnostic {
                        level: DiagnosticLevel::Error,
                        file: file_name.clone(),
                        message: format!(
                            "RPC '{}.{}' references unresolved output message type '{}'",
                            s.name, rpc.name, rpc.output_type
                        ),
                    });
                }
            }
        }
    }

    Ok(SchemaBundle {
        files: parsed_files,
        type_registry,
        diagnostics,
    })
}

fn resolve_field_type(
    ft: &mut FieldType,
    msg_full_name: &str,
    pkg: &str,
    file_name: &str,
    field_name: &str,
    reg: &BTreeMap<String, ResolvedKind>,
    diags: &mut Vec<Diagnostic>,
) {
    match ft {
        FieldType::Scalar(_) => {}
        FieldType::Map(k, v) => {
            resolve_field_type(k, msg_full_name, pkg, file_name, field_name, reg, diags);
            resolve_field_type(v, msg_full_name, pkg, file_name, field_name, reg, diags);
        }
        FieldType::Named {
            raw,
            resolved_fqn,
            kind,
        } => {
            let clean = raw.trim_start_matches('.');
            let candidates = [
                format!("{}.{}", msg_full_name, clean),
                if pkg.is_empty() {
                    clean.to_string()
                } else {
                    format!("{}.{}", pkg, clean)
                },
                clean.to_string(),
            ];
            let mut matched = None;
            for c in &candidates {
                if let Some(&k) = reg.get(c) {
                    matched = Some((c.clone(), k));
                    break;
                }
            }
            if let Some((fqn, k)) = matched {
                *resolved_fqn = fqn;
                *kind = k;
            } else {
                diags.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    file: file_name.to_string(),
                    message: format!(
                        "Field '{}.{}' references unknown type '{}'",
                        msg_full_name, field_name, raw
                    ),
                });
            }
        }
    }
}

fn resolve_and_validate_message(
    msg: &mut Message,
    pkg: &str,
    file_name: &str,
    reg: &BTreeMap<String, ResolvedKind>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut seen_numbers = BTreeSet::new();
    let mut seen_names = BTreeSet::new();

    for f in &mut msg.fields {
        if f.number <= 0 || (19000..=19999).contains(&f.number) {
            diags.push(Diagnostic {
                level: DiagnosticLevel::Error,
                file: file_name.to_string(),
                message: format!(
                    "Field '{}.{}' uses invalid/reserved field number {}",
                    msg.full_name, f.name, f.number
                ),
            });
        }
        if !seen_numbers.insert(f.number) {
            diags.push(Diagnostic {
                level: DiagnosticLevel::Error,
                file: file_name.to_string(),
                message: format!(
                    "Duplicate field number {} in message '{}'",
                    f.number, msg.full_name
                ),
            });
        }
        if !seen_names.insert(f.name.clone()) {
            diags.push(Diagnostic {
                level: DiagnosticLevel::Error,
                file: file_name.to_string(),
                message: format!(
                    "Duplicate field name '{}' in message '{}'",
                    f.name, msg.full_name
                ),
            });
        }
        resolve_field_type(&mut f.field_type, &msg.full_name, pkg, file_name, &f.name, reg, diags);
    }

    for o in &mut msg.oneofs {
        for f in &mut o.fields {
            if !seen_numbers.insert(f.number) {
                diags.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    file: file_name.to_string(),
                    message: format!(
                        "Duplicate field number {} in oneof '{}.{}'",
                        f.number, msg.full_name, o.name
                    ),
                });
            }
            resolve_field_type(&mut f.field_type, &msg.full_name, pkg, file_name, &f.name, reg, diags);
        }
    }

    for nm in &mut msg.nested_messages {
        resolve_and_validate_message(nm, pkg, file_name, reg, diags);
    }
}
