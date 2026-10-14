/*
 * quarto-test/src/assertions/prints_message.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Assertion that checks for specific log messages.
 */

//! `printsMessage` assertion implementation.

use anyhow::{Context, Result, bail};
use regex::Regex;

use super::{Assertion, LogLevel, VerifyContext};

/// Assertion that verifies a specific message was (or wasn't) logged.
///
/// This corresponds to Quarto 1's `printsMessage` verification function.
#[derive(Debug)]
pub struct PrintsMessage {
    /// Log level to match.
    level: LogLevel,
    /// Regex pattern to match against the message.
    regex: Regex,
    /// Original pattern string for error messages.
    pattern: String,
    /// If true, assert the message does NOT appear.
    negate: bool,
}

impl PrintsMessage {
    /// Create a new assertion.
    ///
    /// # Arguments
    /// * `level` - Log level to match (DEBUG, INFO, WARN, ERROR)
    /// * `pattern` - Regex pattern to match against messages
    /// * `negate` - If true, asserts the pattern does NOT match
    pub fn new(level: LogLevel, pattern: String, negate: bool) -> Result<Self> {
        let regex =
            Regex::new(&pattern).with_context(|| format!("invalid regex pattern: {}", pattern))?;

        Ok(Self {
            level,
            regex,
            pattern,
            negate,
        })
    }

    /// Parse log level from string.
    pub fn parse_level(s: &str) -> Result<LogLevel> {
        match s.to_uppercase().as_str() {
            "DEBUG" => Ok(LogLevel::Debug),
            "INFO" => Ok(LogLevel::Info),
            "WARN" | "WARNING" => Ok(LogLevel::Warn),
            "ERROR" => Ok(LogLevel::Error),
            _ => bail!("unknown log level: {}", s),
        }
    }
}

impl Assertion for PrintsMessage {
    fn name(&self) -> &str {
        "printsMessage"
    }

    fn verify(&self, context: &VerifyContext) -> Result<()> {
        // Check if any message matches the level and pattern
        let found = context
            .messages
            .iter()
            .any(|msg| msg.level == self.level && self.regex.is_match(&msg.message));

        if self.negate {
            // We expect NOT to find the message
            if found {
                bail!(
                    "Found unexpected {} message matching '{}'\nMessages:\n{}",
                    self.level.as_str(),
                    self.pattern,
                    format_messages(&context.messages)
                );
            }
        } else {
            // We expect to find the message
            if !found {
                bail!(
                    "Missing {} message matching '{}'\nMessages:\n{}",
                    self.level.as_str(),
                    self.pattern,
                    format_messages(&context.messages)
                );
            }
        }

        Ok(())
    }
}

/// Format messages for error output.
fn format_messages(messages: &[super::LogMessage]) -> String {
    if messages.is_empty() {
        "  (no messages captured)".to_string()
    } else {
        messages
            .iter()
            .map(|m| format!("  [{}] {}", m.level.as_str(), m.indented("    ")))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_context(messages: Vec<super::super::LogMessage>) -> VerifyContext {
        VerifyContext {
            output_path: PathBuf::from("/tmp/test.html"),
            input_path: PathBuf::from("/tmp/test.qmd"),
            format: "html".to_string(),
            render_error: None,
            messages,
        }
    }

    #[test]
    fn test_finds_matching_message() {
        let assertion =
            PrintsMessage::new(LogLevel::Error, "missing.*title".to_string(), false).unwrap();

        let context = make_context(vec![super::super::LogMessage {
            level: LogLevel::Error,
            message: "Error: missing document title".to_string(),
        }]);

        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_fails_when_message_not_found() {
        let assertion =
            PrintsMessage::new(LogLevel::Error, "missing.*title".to_string(), false).unwrap();

        let context = make_context(vec![super::super::LogMessage {
            level: LogLevel::Warn,
            message: "Some warning".to_string(),
        }]);

        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Missing ERROR"));
    }

    #[test]
    fn test_negate_passes_when_not_found() {
        let assertion = PrintsMessage::new(
            LogLevel::Error,
            "critical.*failure".to_string(),
            true, // negate
        )
        .unwrap();

        let context = make_context(vec![super::super::LogMessage {
            level: LogLevel::Warn,
            message: "Some warning".to_string(),
        }]);

        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_negate_fails_when_found() {
        let assertion = PrintsMessage::new(
            LogLevel::Error,
            "critical".to_string(),
            true, // negate
        )
        .unwrap();

        let context = make_context(vec![super::super::LogMessage {
            level: LogLevel::Error,
            message: "critical failure occurred".to_string(),
        }]);

        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Found unexpected"));
    }

    #[test]
    fn test_level_must_match() {
        let assertion =
            PrintsMessage::new(LogLevel::Error, "some message".to_string(), false).unwrap();

        // Same message but at WARN level - should not match
        let context = make_context(vec![super::super::LogMessage {
            level: LogLevel::Warn,
            message: "some message here".to_string(),
        }]);

        let result = assertion.verify(&context);
        assert!(result.is_err());
    }
}
