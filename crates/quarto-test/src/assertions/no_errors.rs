/*
 * quarto-test/src/assertions/no_errors.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Assertions for verifying no errors or warnings occurred.
 */

//! `noErrors` and `noErrorsOrWarnings` assertion implementations.

use anyhow::bail;

use super::{Assertion, LogLevel, VerifyContext};

/// Assertion that verifies no errors occurred during rendering.
///
/// This corresponds to Quarto 1's `noErrors` verification function.
/// The test passes if no ERROR-level messages were logged.
#[derive(Debug)]
pub struct NoErrors;

impl NoErrors {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoErrors {
    fn default() -> Self {
        Self::new()
    }
}

impl Assertion for NoErrors {
    fn name(&self) -> &str {
        "noErrors"
    }

    fn verify(&self, context: &VerifyContext) -> anyhow::Result<()> {
        // Check for render error first
        if let Some(err) = &context.render_error {
            bail!("Rendering failed with error: {}", err);
        }

        // Check for ERROR-level messages
        let errors: Vec<String> = context
            .messages
            .iter()
            .filter(|m| matches!(m.level, LogLevel::Error))
            .map(|m| m.indented("    "))
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            bail!(
                "Expected no errors, but found {}:\n  - {}",
                errors.len(),
                errors.join("\n  - ")
            )
        }
    }
}

/// Assertion that verifies no errors or warnings occurred during rendering.
///
/// This corresponds to Quarto 1's `noErrorsOrWarnings` verification function.
/// The test passes if no ERROR or WARN level messages were logged.
#[derive(Debug)]
pub struct NoErrorsOrWarnings;

impl NoErrorsOrWarnings {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoErrorsOrWarnings {
    fn default() -> Self {
        Self::new()
    }
}

impl Assertion for NoErrorsOrWarnings {
    fn name(&self) -> &str {
        "noErrorsOrWarnings"
    }

    fn verify(&self, context: &VerifyContext) -> anyhow::Result<()> {
        // Check for render error first
        if let Some(err) = &context.render_error {
            bail!("Rendering failed with error: {}", err);
        }

        // Check for ERROR or WARN level messages
        let problems: Vec<String> = context
            .messages
            .iter()
            .filter(|m| matches!(m.level, LogLevel::Error | LogLevel::Warn))
            .map(|m| {
                let level = match m.level {
                    LogLevel::Error => "ERROR",
                    LogLevel::Warn => "WARN",
                    _ => "?",
                };
                format!("[{}] {}", level, m.indented("    "))
            })
            .collect();

        if problems.is_empty() {
            Ok(())
        } else {
            bail!(
                "Expected no errors or warnings, but found {}:\n  - {}",
                problems.len(),
                problems.join("\n  - ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assertions::LogMessage;
    use std::path::PathBuf;

    fn make_context(render_error: Option<String>, messages: Vec<LogMessage>) -> VerifyContext {
        VerifyContext {
            output_path: PathBuf::from("/tmp/test.html"),
            input_path: PathBuf::from("/tmp/test.qmd"),
            format: "html".to_string(),
            render_error,
            messages,
        }
    }

    #[test]
    fn test_no_errors_passes_with_no_messages() {
        let assertion = NoErrors::new();
        let context = make_context(None, vec![]);
        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_no_errors_passes_with_warnings() {
        let assertion = NoErrors::new();
        let context = make_context(
            None,
            vec![LogMessage {
                level: LogLevel::Warn,
                message: "Some warning".to_string(),
            }],
        );
        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_no_errors_fails_with_error_message() {
        let assertion = NoErrors::new();
        let context = make_context(
            None,
            vec![LogMessage {
                level: LogLevel::Error,
                message: "Some error".to_string(),
            }],
        );
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Expected no errors")
        );
    }

    #[test]
    fn test_no_errors_fails_with_render_error() {
        let assertion = NoErrors::new();
        let context = make_context(Some("Parse failed".to_string()), vec![]);
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Rendering failed"));
    }

    #[test]
    fn test_no_errors_or_warnings_passes_with_no_messages() {
        let assertion = NoErrorsOrWarnings::new();
        let context = make_context(None, vec![]);
        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_no_errors_or_warnings_passes_with_info() {
        let assertion = NoErrorsOrWarnings::new();
        let context = make_context(
            None,
            vec![LogMessage {
                level: LogLevel::Info,
                message: "Some info".to_string(),
            }],
        );
        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_no_errors_or_warnings_fails_with_warning() {
        let assertion = NoErrorsOrWarnings::new();
        let context = make_context(
            None,
            vec![LogMessage {
                level: LogLevel::Warn,
                message: "Some warning".to_string(),
            }],
        );
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Expected no errors or warnings")
        );
    }

    #[test]
    fn test_no_errors_or_warnings_fails_with_error() {
        let assertion = NoErrorsOrWarnings::new();
        let context = make_context(
            None,
            vec![LogMessage {
                level: LogLevel::Error,
                message: "Some error".to_string(),
            }],
        );
        let result = assertion.verify(&context);
        assert!(result.is_err());
    }

    #[test]
    fn test_no_errors_or_warnings_fails_with_render_error() {
        let assertion = NoErrorsOrWarnings::new();
        let context = make_context(Some("Parse failed".to_string()), vec![]);
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Rendering failed"));
    }
}
