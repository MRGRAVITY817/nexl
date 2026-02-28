//! WIT import — parse WIT interface files and generate Nexl type bindings.
//!
//! Implements the `wit-import` feature (M23 task 5):
//! - Tokenize and parse WIT interface declarations into a `WitInterface` AST.
//! - Convert WIT types to Nexl [`Type`] values via [`wit_type_to_nexl`].
//! - Produce [`WitImportBinding`] entries usable by the type-checker and evaluator.
//!
//! Supported WIT subset:
//! - `package` declarations (skipped)
//! - `interface name { … }` with functions and resources
//! - Primitive types: s8/s16/s32/s64, u8/u16/u32/u64, float32/float64, bool, string, unit
//! - Composite types: `list<T>`, `record { … }`
//! - Resource definitions with methods
//! - Named type references (mapped to `Type::Adt`)

use nexl_types::{EffectRow, Type};

// ─── Public types ────────────────────────────────────────────────────────────

/// A primitive or composite WIT type parsed from a `.wit` file.
#[derive(Debug, Clone, PartialEq)]
pub enum WitType {
    // Signed integers
    S8,
    S16,
    S32,
    S64,
    // Unsigned integers
    U8,
    U16,
    U32,
    U64,
    // Floats
    Float32,
    Float64,
    // Other primitives
    Bool,
    String,
    Unit,
    // Composite
    /// `list<T>`
    List(Box<WitType>),
    /// `record { field: type, … }`
    Record { fields: Vec<(std::string::String, WitType)> },
    /// A named type reference (resource handle, user-defined type, etc.).
    Named(std::string::String),
}

/// A named parameter in a WIT function signature.
#[derive(Debug, Clone, PartialEq)]
pub struct WitParam {
    /// Parameter name (e.g. `"p0"`, `"path"`).
    pub name: std::string::String,
    /// Parameter type.
    pub ty: WitType,
}

/// A WIT function signature.
#[derive(Debug, Clone, PartialEq)]
pub struct WitFn {
    /// Function name in kebab-case (e.g. `"read-file"`).
    pub name: std::string::String,
    /// Parameter list.
    pub params: Vec<WitParam>,
    /// Return type — `None` when the function returns `unit`.
    pub result: Option<WitType>,
}

/// A WIT resource definition with its drop method and other methods.
#[derive(Debug, Clone, PartialEq)]
pub struct WitResourceDef {
    /// Resource name (e.g. `"connection"`).
    pub name: std::string::String,
    /// Methods on this resource.
    pub methods: Vec<WitFn>,
}

/// A fully parsed WIT interface declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct WitInterface {
    /// Interface name (e.g. `"filesystem"`).
    pub name: std::string::String,
    /// Free functions in this interface.
    pub functions: Vec<WitFn>,
    /// Resource types defined in this interface.
    pub resources: Vec<WitResourceDef>,
}

/// Errors produced by WIT import operations.
#[derive(Debug, Clone, PartialEq)]
pub enum WitImportError {
    /// Syntax error while parsing WIT text.
    ParseError(std::string::String),
    /// A WIT type has no Nexl equivalent.
    UnknownType(std::string::String),
}

impl std::fmt::Display for WitImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WitImportError::ParseError(msg) => write!(f, "WIT parse error: {msg}"),
            WitImportError::UnknownType(t) => write!(f, "unknown WIT type: {t}"),
        }
    }
}

impl std::error::Error for WitImportError {}

/// A Nexl type binding generated from a WIT function.
#[derive(Debug, Clone, PartialEq)]
pub struct WitImportBinding {
    /// Function name (kebab-case, as in the WIT file).
    pub name: std::string::String,
    /// Nexl function type for this binding.
    pub ty: Type,
}

// ─── Tokenizer ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(std::string::String),
    Punct(char),
    /// The `->` arrow (return-type separator).
    Arrow,
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        // Skip whitespace.
        if ch.is_whitespace() {
            i += 1;
            continue;
        }

        // Skip line comments (`// …`).
        if ch == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Arrow `->` — must come before ident check so `-` isn't consumed.
        if ch == '-' && i + 1 < chars.len() && chars[i + 1] == '>' {
            tokens.push(Token::Arrow);
            i += 2;
            continue;
        }

        // Identifier: starts with alphanumeric or `_`; continues with alphanumeric, `_`, or `-`
        // (WIT uses kebab-case names like `read-file`).
        if ch.is_alphanumeric() || ch == '_' {
            let start = i;
            i += 1;
            while i < chars.len() {
                let c = chars[i];
                if c.is_alphanumeric() || c == '_' {
                    i += 1;
                } else if c == '-' {
                    // Only include `-` in the ident if it does NOT form `->`.
                    if i + 1 < chars.len() && chars[i + 1] == '>' {
                        break;
                    }
                    i += 1;
                } else {
                    break;
                }
            }
            let ident: std::string::String = chars[start..i].iter().collect();
            tokens.push(Token::Ident(ident));
            continue;
        }

        // Single-character punctuation.
        if "{}()<>,;:=@".contains(ch) {
            tokens.push(Token::Punct(ch));
            i += 1;
            continue;
        }

        // Skip anything else (e.g. `%`, `?`, `!`).
        i += 1;
    }

    tokens
}

// ─── Parser ──────────────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(tok)
        } else {
            None
        }
    }

    fn expect_ident(&mut self) -> Result<std::string::String, WitImportError> {
        match self.next() {
            Some(Token::Ident(s)) => Ok(s),
            Some(t) => Err(WitImportError::ParseError(format!(
                "expected identifier, got {t:?}"
            ))),
            None => Err(WitImportError::ParseError(
                "unexpected end of input, expected identifier".to_string(),
            )),
        }
    }

    fn expect_punct(&mut self, ch: char) -> Result<(), WitImportError> {
        match self.next() {
            Some(Token::Punct(c)) if c == ch => Ok(()),
            Some(t) => Err(WitImportError::ParseError(format!(
                "expected '{ch}', got {t:?}"
            ))),
            None => Err(WitImportError::ParseError(format!(
                "unexpected end of input, expected '{ch}'"
            ))),
        }
    }

    fn peek_punct(&self, ch: char) -> bool {
        matches!(self.peek(), Some(Token::Punct(c)) if *c == ch)
    }

    fn peek_ident(&self) -> Option<&str> {
        match self.peek() {
            Some(Token::Ident(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Skip tokens until (and including) a `;`.
    fn skip_to_semicolon(&mut self) {
        while let Some(tok) = self.peek() {
            if matches!(tok, Token::Punct(';')) {
                self.next();
                return;
            }
            self.next();
        }
    }

    /// Skip a balanced block `{ … }` (consuming both braces).
    fn skip_block(&mut self) -> Result<(), WitImportError> {
        self.expect_punct('{')?;
        let mut depth = 1usize;
        while let Some(tok) = self.next() {
            match tok {
                Token::Punct('{') => depth += 1,
                Token::Punct('}') => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        Err(WitImportError::ParseError(
            "unclosed block `{`".to_string(),
        ))
    }

    /// Parse a WIT type.
    fn parse_type(&mut self) -> Result<WitType, WitImportError> {
        match self.peek() {
            Some(Token::Ident(s)) => {
                let s = s.clone();
                match s.as_str() {
                    "s8" => {
                        self.next();
                        Ok(WitType::S8)
                    }
                    "s16" => {
                        self.next();
                        Ok(WitType::S16)
                    }
                    "s32" => {
                        self.next();
                        Ok(WitType::S32)
                    }
                    "s64" => {
                        self.next();
                        Ok(WitType::S64)
                    }
                    "u8" => {
                        self.next();
                        Ok(WitType::U8)
                    }
                    "u16" => {
                        self.next();
                        Ok(WitType::U16)
                    }
                    "u32" => {
                        self.next();
                        Ok(WitType::U32)
                    }
                    "u64" => {
                        self.next();
                        Ok(WitType::U64)
                    }
                    "float32" => {
                        self.next();
                        Ok(WitType::Float32)
                    }
                    "float64" => {
                        self.next();
                        Ok(WitType::Float64)
                    }
                    "bool" => {
                        self.next();
                        Ok(WitType::Bool)
                    }
                    "string" => {
                        self.next();
                        Ok(WitType::String)
                    }
                    "unit" => {
                        self.next();
                        Ok(WitType::Unit)
                    }
                    "list" => {
                        self.next(); // consume "list"
                        self.expect_punct('<')?;
                        let elem = self.parse_type()?;
                        self.expect_punct('>')?;
                        Ok(WitType::List(Box::new(elem)))
                    }
                    "record" => {
                        self.next(); // consume "record"
                        self.expect_punct('{')?;
                        let mut fields = Vec::new();
                        while !self.peek_punct('}') {
                            if !fields.is_empty() && self.peek_punct(',') {
                                self.next();
                            }
                            if self.peek_punct('}') {
                                break;
                            }
                            let field_name = self.expect_ident()?;
                            self.expect_punct(':')?;
                            let field_type = self.parse_type()?;
                            fields.push((field_name, field_type));
                        }
                        self.expect_punct('}')?;
                        Ok(WitType::Record { fields })
                    }
                    _ => {
                        // Named type (resource reference or user-defined type).
                        self.next();
                        Ok(WitType::Named(s))
                    }
                }
            }
            Some(t) => Err(WitImportError::ParseError(format!(
                "expected type, got {t:?}"
            ))),
            None => Err(WitImportError::ParseError(
                "unexpected end of input in type position".to_string(),
            )),
        }
    }

    /// Parse `name: func(params) -> ret ;`
    fn parse_func(&mut self) -> Result<WitFn, WitImportError> {
        let name = self.expect_ident()?;
        self.expect_punct(':')?;

        // Expect the `func` keyword.
        match self.next() {
            Some(Token::Ident(s)) if s == "func" => {}
            Some(t) => {
                return Err(WitImportError::ParseError(format!(
                    "expected 'func', got {t:?}"
                )))
            }
            None => {
                return Err(WitImportError::ParseError(
                    "expected 'func'".to_string(),
                ))
            }
        }
        self.expect_punct('(')?;

        // Parse parameter list.
        let mut params = Vec::new();
        while !self.peek_punct(')') {
            if !params.is_empty() && self.peek_punct(',') {
                self.next();
            }
            if self.peek_punct(')') {
                break;
            }
            let param_name = self.expect_ident()?;
            self.expect_punct(':')?;
            let param_type = self.parse_type()?;
            params.push(WitParam {
                name: param_name,
                ty: param_type,
            });
        }
        self.expect_punct(')')?;

        // Optional `-> return-type`.
        let result = if matches!(self.peek(), Some(Token::Arrow)) {
            self.next(); // consume `->`
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect_punct(';')?;

        Ok(WitFn {
            name,
            params,
            result,
        })
    }

    /// Parse an `interface name { … }` declaration.
    fn parse_interface(&mut self) -> Result<WitInterface, WitImportError> {
        let name = self.expect_ident()?;
        self.expect_punct('{')?;

        let mut functions = Vec::new();
        let mut resources = Vec::new();

        while !self.peek_punct('}') {
            match self.peek_ident() {
                Some("resource") => {
                    self.next(); // consume "resource"
                    let res_name = self.expect_ident()?;
                    self.expect_punct('{')?;
                    let mut methods = Vec::new();
                    while !self.peek_punct('}') {
                        methods.push(self.parse_func()?);
                    }
                    self.expect_punct('}')?;
                    resources.push(WitResourceDef {
                        name: res_name,
                        methods,
                    });
                }
                // Skip `record`, `variant`, `flags`, `enum`, `type`, `use` definitions
                // at the interface level — they're type aliases we don't need to parse
                // fully for binding generation (named references suffice).
                Some("record") | Some("variant") | Some("flags") | Some("enum") => {
                    self.next(); // consume keyword
                    self.expect_ident()?; // consume name
                    self.skip_block()?;
                }
                Some("type") | Some("use") | Some("include") => {
                    self.skip_to_semicolon();
                }
                Some(_) => {
                    functions.push(self.parse_func()?);
                }
                None => {
                    return Err(WitImportError::ParseError(
                        "unexpected end of input inside interface body".to_string(),
                    ))
                }
            }
        }
        self.expect_punct('}')?;

        Ok(WitInterface {
            name,
            functions,
            resources,
        })
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Parse a WIT interface declaration from text.
///
/// Handles `package` headers, `interface` blocks, free functions, resources,
/// and skips over type definitions (`record`, `variant`, `flags`, `enum`).
///
/// # Errors
/// Returns [`WitImportError::ParseError`] on malformed WIT syntax.
pub fn parse_wit_interface(text: &str) -> Result<WitInterface, WitImportError> {
    let tokens = tokenize(text);
    let mut parser = Parser::new(tokens);

    // Skip top-level declarations until we find `interface`.
    loop {
        match parser.peek() {
            None => {
                return Err(WitImportError::ParseError(
                    "no `interface` declaration found".to_string(),
                ))
            }
            Some(Token::Ident(s)) if s == "package" => {
                parser.next();
                parser.skip_to_semicolon();
            }
            Some(Token::Ident(s)) if s == "use" => {
                parser.skip_to_semicolon();
            }
            Some(Token::Ident(s)) if s == "interface" => {
                parser.next(); // consume "interface"
                return parser.parse_interface();
            }
            _ => {
                // Skip unexpected top-level tokens.
                parser.next();
            }
        }
    }
}

/// Convert a [`WitType`] to its Nexl [`Type`] equivalent.
///
/// | WIT type     | Nexl type              |
/// |--------------|------------------------|
/// | `s8`         | [`Type::Int8`]         |
/// | `s16`        | [`Type::Int16`]        |
/// | `s32`        | [`Type::Int32`]        |
/// | `s64`        | [`Type::Int`]          |
/// | `u8`–`u64`   | [`Type::U8`]–[`Type::U64`] |
/// | `float32`    | [`Type::F32`]          |
/// | `float64`    | [`Type::Float`]        |
/// | `bool`       | [`Type::Bool`]         |
/// | `string`     | [`Type::Str`]          |
/// | `unit`       | [`Type::Unit`]         |
/// | `list<T>`    | `Type::Vec(T)`         |
/// | `record { …}`| `Type::Record { … }`   |
/// | `Named(n)`   | `Type::Adt { name: n }` |
///
/// # Errors
/// Returns [`WitImportError::UnknownType`] for types that cannot be represented
/// in Nexl's type system.
pub fn wit_type_to_nexl(wt: &WitType) -> Result<Type, WitImportError> {
    match wt {
        WitType::S8 => Ok(Type::Int8),
        WitType::S16 => Ok(Type::Int16),
        WitType::S32 => Ok(Type::Int32),
        WitType::S64 => Ok(Type::Int),
        WitType::U8 => Ok(Type::U8),
        WitType::U16 => Ok(Type::U16),
        WitType::U32 => Ok(Type::U32),
        WitType::U64 => Ok(Type::U64),
        WitType::Float32 => Ok(Type::F32),
        WitType::Float64 => Ok(Type::Float),
        WitType::Bool => Ok(Type::Bool),
        WitType::String => Ok(Type::Str),
        WitType::Unit => Ok(Type::Unit),
        WitType::List(elem) => {
            let nexl_elem = wit_type_to_nexl(elem)?;
            Ok(Type::Vec(Box::new(nexl_elem)))
        }
        WitType::Record { fields } => {
            let mut nexl_fields = Vec::new();
            for (name, ty) in fields {
                nexl_fields.push((name.clone(), wit_type_to_nexl(ty)?));
            }
            Ok(Type::Record {
                name: "_anon".to_string(),
                fields: nexl_fields,
            })
        }
        WitType::Named(name) => Ok(Type::Adt {
            name: name.clone(),
            args: Vec::new(),
        }),
    }
}

/// Convert a parsed [`WitInterface`] to a list of [`WitImportBinding`]s.
///
/// Each free function in the interface becomes one binding with a `Type::Fn`
/// type. Resources each produce a single binding carrying the opaque
/// `Type::Adt` for the resource type (methods are not individually bound here
/// — they are accessible via the resource handle at runtime).
///
/// # Errors
/// Propagates [`WitImportError::UnknownType`] if any type in the interface
/// cannot be represented in Nexl.
pub fn wit_interface_to_bindings(
    iface: &WitInterface,
) -> Result<Vec<WitImportBinding>, WitImportError> {
    let mut bindings = Vec::new();

    for func in &iface.functions {
        let params: Result<Vec<Type>, _> =
            func.params.iter().map(|p| wit_type_to_nexl(&p.ty)).collect();
        let params = params?;

        let ret = match &func.result {
            Some(t) => wit_type_to_nexl(t)?,
            None => Type::Unit,
        };

        let ty = Type::Fn {
            params,
            ret: Box::new(ret),
            effects: EffectRow::empty(),
        };

        bindings.push(WitImportBinding {
            name: func.name.clone(),
            ty,
        });
    }

    // Resources → opaque Adt type binding.
    for res in &iface.resources {
        bindings.push(WitImportBinding {
            name: res.name.clone(),
            ty: Type::Adt {
                name: res.name.clone(),
                args: Vec::new(),
            },
        });
    }

    Ok(bindings)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test 1 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_primitive_wit_types() {
        // s64, string, bool, float64 → correct WitType variants via parse_type
        let primitives = [
            ("s8", WitType::S8),
            ("s16", WitType::S16),
            ("s32", WitType::S32),
            ("s64", WitType::S64),
            ("u8", WitType::U8),
            ("u16", WitType::U16),
            ("u32", WitType::U32),
            ("u64", WitType::U64),
            ("float32", WitType::Float32),
            ("float64", WitType::Float64),
            ("bool", WitType::Bool),
            ("string", WitType::String),
            ("unit", WitType::Unit),
        ];
        for (name, expected) in &primitives {
            // Wrap in a minimal interface/function to exercise the parser.
            let wit = format!(
                "interface foo {{ bar: func(p: {name}) -> {name}; }}"
            );
            let iface = parse_wit_interface(&wit)
                .unwrap_or_else(|e| panic!("parse failed for {name}: {e}"));
            assert_eq!(
                iface.functions[0].params[0].ty,
                *expected,
                "param type for {name}"
            );
            assert_eq!(
                iface.functions[0].result,
                Some(expected.clone()),
                "return type for {name}"
            );
        }
    }

    // ── Test 2 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_simple_interface() {
        let wit = r#"
            interface math {
                add: func(a: s64, b: s64) -> s64;
            }
        "#;
        let iface = parse_wit_interface(wit).unwrap();
        assert_eq!(iface.name, "math");
        assert_eq!(iface.functions.len(), 1);
        let f = &iface.functions[0];
        assert_eq!(f.name, "add");
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].name, "a");
        assert_eq!(f.params[0].ty, WitType::S64);
        assert_eq!(f.params[1].name, "b");
        assert_eq!(f.params[1].ty, WitType::S64);
        assert_eq!(f.result, Some(WitType::S64));
    }

    // ── Test 3 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_multiple_functions() {
        let wit = r#"
            interface string-utils {
                reverse: func(s: string) -> string;
                length: func(s: string) -> u32;
                contains: func(haystack: string, needle: string) -> bool;
            }
        "#;
        let iface = parse_wit_interface(wit).unwrap();
        assert_eq!(iface.name, "string-utils");
        assert_eq!(iface.functions.len(), 3);
        assert_eq!(iface.functions[0].name, "reverse");
        assert_eq!(iface.functions[1].name, "length");
        assert_eq!(iface.functions[2].name, "contains");
        assert_eq!(iface.functions[2].params.len(), 2);
        assert_eq!(iface.functions[2].result, Some(WitType::Bool));
    }

    // ── Test 4 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_unit_return_function() {
        let wit = r#"
            interface io {
                print: func(msg: string);
                flush: func();
            }
        "#;
        let iface = parse_wit_interface(wit).unwrap();
        assert_eq!(iface.functions.len(), 2);
        assert_eq!(iface.functions[0].result, None, "print has no return type");
        assert_eq!(iface.functions[1].result, None, "flush has no return type");
        assert_eq!(iface.functions[1].params.len(), 0, "flush takes no params");
    }

    // ── Test 5 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_list_type() {
        let wit = r#"
            interface collections {
                sum: func(items: list<s64>) -> s64;
                bytes: func(n: u32) -> list<u8>;
            }
        "#;
        let iface = parse_wit_interface(wit).unwrap();
        // sum's first param is list<s64>
        assert_eq!(
            iface.functions[0].params[0].ty,
            WitType::List(Box::new(WitType::S64))
        );
        // bytes returns list<u8>
        assert_eq!(
            iface.functions[1].result,
            Some(WitType::List(Box::new(WitType::U8)))
        );
    }

    // ── Test 6 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_record_type() {
        // Inline record in a function parameter.
        let wit = r#"
            interface db {
                insert: func(row: record { id: u32, name: string }) -> bool;
            }
        "#;
        let iface = parse_wit_interface(wit).unwrap();
        let param_ty = &iface.functions[0].params[0].ty;
        match param_ty {
            WitType::Record { fields } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].0, "id");
                assert_eq!(fields[0].1, WitType::U32);
                assert_eq!(fields[1].0, "name");
                assert_eq!(fields[1].1, WitType::String);
            }
            other => panic!("expected Record, got {other:?}"),
        }
    }

    // ── Test 7 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_resource() {
        let wit = r#"
            interface database {
                resource connection {
                    query: func(sql: string) -> list<string>;
                    close: func();
                }
            }
        "#;
        let iface = parse_wit_interface(wit).unwrap();
        assert_eq!(iface.functions.len(), 0, "no free functions");
        assert_eq!(iface.resources.len(), 1);
        let res = &iface.resources[0];
        assert_eq!(res.name, "connection");
        assert_eq!(res.methods.len(), 2);
        assert_eq!(res.methods[0].name, "query");
        assert_eq!(
            res.methods[0].result,
            Some(WitType::List(Box::new(WitType::String)))
        );
        assert_eq!(res.methods[1].name, "close");
        assert_eq!(res.methods[1].result, None);
    }

    // ── Test 8 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_package_header() {
        // Package declarations are skipped cleanly.
        let wit = r#"
            package wasi:io@0.2.0;

            interface streams {
                write: func(buf: list<u8>) -> u32;
            }
        "#;
        let iface = parse_wit_interface(wit).unwrap();
        assert_eq!(iface.name, "streams");
        assert_eq!(iface.functions.len(), 1);
    }

    // ── Test 9 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_wit_type_to_nexl_primitives() {
        assert_eq!(wit_type_to_nexl(&WitType::S8).unwrap(), Type::Int8);
        assert_eq!(wit_type_to_nexl(&WitType::S16).unwrap(), Type::Int16);
        assert_eq!(wit_type_to_nexl(&WitType::S32).unwrap(), Type::Int32);
        assert_eq!(wit_type_to_nexl(&WitType::S64).unwrap(), Type::Int);
        assert_eq!(wit_type_to_nexl(&WitType::U8).unwrap(), Type::U8);
        assert_eq!(wit_type_to_nexl(&WitType::U16).unwrap(), Type::U16);
        assert_eq!(wit_type_to_nexl(&WitType::U32).unwrap(), Type::U32);
        assert_eq!(wit_type_to_nexl(&WitType::U64).unwrap(), Type::U64);
        assert_eq!(wit_type_to_nexl(&WitType::Float32).unwrap(), Type::F32);
        assert_eq!(wit_type_to_nexl(&WitType::Float64).unwrap(), Type::Float);
        assert_eq!(wit_type_to_nexl(&WitType::Bool).unwrap(), Type::Bool);
        assert_eq!(wit_type_to_nexl(&WitType::String).unwrap(), Type::Str);
        assert_eq!(wit_type_to_nexl(&WitType::Unit).unwrap(), Type::Unit);
    }

    // ── Test 10 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_wit_type_to_nexl_list() {
        let wt = WitType::List(Box::new(WitType::S64));
        let nexl = wit_type_to_nexl(&wt).unwrap();
        assert_eq!(nexl, Type::Vec(Box::new(Type::Int)));
    }

    // ── Test 11 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_wit_type_to_nexl_record() {
        let wt = WitType::Record {
            fields: vec![
                ("name".to_string(), WitType::String),
                ("age".to_string(), WitType::U32),
            ],
        };
        let nexl = wit_type_to_nexl(&wt).unwrap();
        match nexl {
            Type::Record { name, fields } => {
                assert_eq!(name, "_anon");
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0], ("name".to_string(), Type::Str));
                assert_eq!(fields[1], ("age".to_string(), Type::U32));
            }
            other => panic!("expected Record, got {other:?}"),
        }
    }

    // ── Test 12 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_wit_type_to_nexl_resource() {
        // Named types (resource references) → opaque Adt.
        let wt = WitType::Named("connection".to_string());
        let nexl = wit_type_to_nexl(&wt).unwrap();
        assert_eq!(
            nexl,
            Type::Adt {
                name: "connection".to_string(),
                args: Vec::new(),
            }
        );
    }

    // ── Test 13 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_interface_to_bindings() {
        let wit = r#"
            interface math {
                add: func(a: s64, b: s64) -> s64;
                to-float: func(n: s64) -> float64;
                print-int: func(n: s64);
            }
        "#;
        let iface = parse_wit_interface(wit).unwrap();
        let bindings = wit_interface_to_bindings(&iface).unwrap();
        assert_eq!(bindings.len(), 3);

        // add: (Fn [Int Int] -> Int)
        assert_eq!(bindings[0].name, "add");
        assert_eq!(
            bindings[0].ty,
            Type::Fn {
                params: vec![Type::Int, Type::Int],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }
        );

        // to-float: (Fn [Int] -> Float)
        assert_eq!(bindings[1].name, "to-float");
        assert_eq!(
            bindings[1].ty,
            Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Float),
                effects: EffectRow::empty(),
            }
        );

        // print-int: (Fn [Int] -> Unit)
        assert_eq!(bindings[2].name, "print-int");
        assert_eq!(
            bindings[2].ty,
            Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Unit),
                effects: EffectRow::empty(),
            }
        );
    }

    // ── Test 14 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_syntax_error() {
        // Truncated WIT — missing closing brace.
        let wit = "interface foo { bar: func(p0: string)";
        let result = parse_wit_interface(wit);
        assert!(
            matches!(result, Err(WitImportError::ParseError(_))),
            "expected ParseError, got {result:?}"
        );
    }
}
