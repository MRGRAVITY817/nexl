pub mod codes;

use nexl_ast::Span;
use std::fmt;

/// Severity level of a compiler diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A hard error: compilation cannot succeed.
    Error,
    /// A recoverable issue: compilation continues but the output may be wrong.
    Warning,
    /// Informational annotation with no impact on compilation.
    Note,
    /// A suggested fix or additional guidance.
    Help,
}

/// A source-location annotation attached to a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

impl Label {
    /// Create a label pointing at `span` with the given message.
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

/// The compiler phase that produced an error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorPhase {
    /// Errors from the lexer (tokeniser).
    Lexer,
    /// Errors from the reader (s-expression parser).
    Reader,
}

/// A structured error code, e.g. `NXL-L0001`.
///
/// Formatted as `NXL-<phase-letter><4-digit-number>`: `NXL-L0001`, `NXL-R0042`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorCode {
    pub phase: ErrorPhase,
    pub number: u32,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let phase = match self.phase {
            ErrorPhase::Lexer => 'L',
            ErrorPhase::Reader => 'R',
        };
        write!(f, "NXL-{phase}{:04}", self.number)
    }
}

/// A compiler diagnostic: a structured, renderable error or warning.
///
/// Implements [`miette::Diagnostic`] so it can be converted to a
/// [`miette::Report`] for terminal rendering with source snippets.
#[derive(Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: Option<ErrorCode>,
    pub message: String,
    pub labels: Vec<Label>,
    pub help: Option<String>,
    pub notes: Vec<String>,
    /// Source text used to render span highlights. Set via [`Diagnostic::attach_source`].
    source: Option<miette::NamedSource<String>>,
}

impl Diagnostic {
    /// Create a diagnostic with the given severity and message.
    /// All other fields default to empty/absent.
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            code: None,
            message: message.into(),
            labels: Vec::new(),
            help: None,
            notes: Vec::new(),
            source: None,
        }
    }

    /// Attach source text so that span labels can be rendered with context.
    pub fn attach_source(&mut self, source: miette::NamedSource<String>) {
        self.source = Some(source);
    }

    /// Append a span label to this diagnostic.
    pub fn push_label(&mut self, label: Label) {
        self.labels.push(label);
    }

    /// Set the help text shown below the diagnostic.
    pub fn set_help(&mut self, help: impl Into<String>) {
        self.help = Some(help.into());
    }

    /// Append an informational note to this diagnostic.
    pub fn add_note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Diagnostic {}

impl miette::Diagnostic for Diagnostic {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.code
            .as_ref()
            .map(|c| Box::new(c) as Box<dyn fmt::Display>)
    }

    fn severity(&self) -> Option<miette::Severity> {
        Some(match self.severity {
            Severity::Error => miette::Severity::Error,
            Severity::Warning => miette::Severity::Warning,
            Severity::Note | Severity::Help => miette::Severity::Advice,
        })
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.help
            .as_ref()
            .map(|h| Box::new(h) as Box<dyn fmt::Display>)
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        self.source.as_ref().map(|s| s as &dyn miette::SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        if self.labels.is_empty() {
            return None;
        }
        Some(Box::new(self.labels.iter().map(|l| {
            miette::LabeledSpan::new_with_span(
                Some(l.message.clone()),
                (l.span.start as usize, l.span.len as usize),
            )
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use nexl_ast::{FileId, Span};

    #[test]
    fn severity_variants_exist() {
        let _variants = [
            Severity::Error,
            Severity::Warning,
            Severity::Note,
            Severity::Help,
        ];
        match Severity::Error {
            Severity::Error | Severity::Warning | Severity::Note | Severity::Help => {}
        }
    }

    #[test]
    fn diagnostic_minimum_construction() {
        let d = Diagnostic::new(Severity::Error, "unexpected token");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.message, "unexpected token");
        assert!(d.labels.is_empty());
        assert!(d.help.is_none());
        assert!(d.notes.is_empty());
        assert!(d.code.is_none());
    }

    #[test]
    fn diagnostic_add_label() {
        let span = Span::new(FileId(0), 4, 6);
        let mut d = Diagnostic::new(Severity::Error, "type mismatch");
        d.push_label(Label::new(span, "expected Int"));
        assert_eq!(d.labels.len(), 1);
        assert_eq!(d.labels[0].message, "expected Int");
    }

    #[test]
    fn diagnostic_with_help_text() {
        let mut d = Diagnostic::new(Severity::Warning, "unused variable");
        d.set_help("prefix the name with `_` to suppress this warning");
        assert_eq!(
            d.help.as_deref(),
            Some("prefix the name with `_` to suppress this warning")
        );
    }

    #[test]
    fn diagnostic_with_notes() {
        let mut d = Diagnostic::new(Severity::Error, "undefined variable");
        d.add_note("declared in outer scope");
        d.add_note("did you mean `count`?");
        assert_eq!(d.notes.len(), 2);
        assert_eq!(d.notes[0], "declared in outer scope");
        assert_eq!(d.notes[1], "did you mean `count`?");
    }

    #[test]
    fn error_code_display() {
        let lexer_code = ErrorCode {
            phase: ErrorPhase::Lexer,
            number: 1,
        };
        assert_eq!(lexer_code.to_string(), "NXL-L0001");

        let reader_code = ErrorCode {
            phase: ErrorPhase::Reader,
            number: 42,
        };
        assert_eq!(reader_code.to_string(), "NXL-R0042");
    }

    #[test]
    fn predefined_codes_accessible() {
        // Each predefined code is a distinct constant with the right phase
        assert_eq!(codes::UNCLOSED_STRING.phase, ErrorPhase::Lexer);
        assert_eq!(codes::INVALID_ESCAPE.phase, ErrorPhase::Lexer);
        assert_eq!(codes::INVALID_CHAR_LITERAL.phase, ErrorPhase::Lexer);
        // They are distinct from each other
        assert_ne!(codes::UNCLOSED_STRING.number, codes::INVALID_ESCAPE.number);
        assert_ne!(
            codes::INVALID_ESCAPE.number,
            codes::INVALID_CHAR_LITERAL.number
        );
    }

    #[test]
    fn diagnostic_implements_std_error() {
        let d = Diagnostic::new(Severity::Error, "oops");
        // Coercing to &dyn std::error::Error proves the trait is implemented.
        let _: &dyn std::error::Error = &d;
    }

    #[test]
    fn miette_report_from_diagnostic() {
        let d = Diagnostic::new(Severity::Error, "unexpected `)`");
        let report = miette::Report::new(d);
        let rendered = format!("{report:?}");
        assert!(rendered.contains("unexpected `)`"));
    }

    #[test]
    fn diagnostic_with_source_and_label_renders() {
        use miette::NamedSource;
        let src = "hello world";
        let span = Span::new(FileId(0), 6, 5); // "world"
        let mut d = Diagnostic::new(Severity::Error, "unexpected word");
        d.attach_source(NamedSource::new("test.nxl", src.to_string()));
        d.push_label(Label::new(span, "this word"));
        let report = miette::Report::new(d);
        let rendered = format!("{report:?}");
        assert!(rendered.contains("unexpected word"));
        assert!(rendered.contains("this word"));
    }

    #[test]
    fn label_holds_span_and_message() {
        let span = Span::new(FileId(0), 10, 5);
        let label = Label::new(span, "expected identifier");
        assert_eq!(label.span, span);
        assert_eq!(label.message, "expected identifier");
    }
}
