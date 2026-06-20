use dialoguer::Password;
use std::io::{self, IsTerminal, Write};

pub fn prompt(label: &str) -> String {
    print!("{label}");
    let _ = io::stdout().flush();
    let mut s = String::new();
    io::stdin().read_line(&mut s).unwrap();
    s.trim().to_string()
}

pub fn prompt_password(label: &str) -> String {
    let prompt_text = label.trim_end_matches(':').trim();
    if io::stdin().is_terminal() {
        Password::new()
            .with_prompt(prompt_text)
            .allow_empty_password(true)
            .interact()
            .unwrap_or_default()
    } else {
        prompt(label)
    }
}
