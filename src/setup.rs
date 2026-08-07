use std::fs;

/// Configures environment variables in the project's `.env` or `.luminus/config.toml`.
pub struct SetupWizard;

impl SetupWizard {
    pub fn save_env_var(
        project_root: &std::path::Path,
        key: &str,
        value: &str,
    ) -> std::io::Result<()> {
        let env_path = project_root.join(".env");
        let content = if env_path.exists() {
            fs::read_to_string(&env_path)?
        } else {
            String::new()
        };

        let target = format!("{}=", key);
        let mut replaced = false;
        let mut new_lines = Vec::new();

        for line in content.lines() {
            if line.starts_with(&target) {
                new_lines.push(format!("{}={}", key, value));
                replaced = true;
            } else {
                new_lines.push(line.to_owned());
            }
        }

        if !replaced {
            new_lines.push(format!("{}={}", key, value));
        }

        new_lines.push(String::new()); // trailing newline
        fs::write(env_path, new_lines.join("\n"))
    }
}
