//! Arrow-key menus and a hidden key prompt for a person at a terminal. They
//! draw on stderr, so stdout stays clean, and scripts never reach them.
use anyhow::Result;
use dialoguer::theme::ColorfulTheme;

/// The chosen row, or `None` when the person presses Escape or q.
pub fn choose(prompt: &str, rows: &[String], default: usize) -> Result<Option<usize>> {
    Ok(dialoguer::Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(rows)
        .default(default.min(rows.len().saturating_sub(1)))
        .max_length(12)
        .interact_opt()?)
}

/// Reads a secret without echoing it.
#[cfg(feature = "download")]
pub fn secret(prompt: &str) -> Result<String> {
    Ok(dialoguer::Password::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .interact()?)
}
