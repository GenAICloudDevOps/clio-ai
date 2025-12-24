mod config;
mod llm;
mod tools;

use config::{Config, MODELS};
use llm::LLM;
use rustyline::DefaultEditor;
use std::env;
use tools::{execute_tool, is_supported_action, ToolCall, ToolResult};

#[tokio::main]
async fn main() {
    let config = Config::load();
    let mut llm = LLM::new(config.clone());
    let cwd = env::current_dir().unwrap();
    let cwd_str = cwd.to_string_lossy().to_string();

    println!("clio-ai v0.1.0 | Model: {} | /help for commands", config.model);

    let mut rl = DefaultEditor::new().unwrap();

    loop {
        let readline = rl.readline(">>> ");
        match readline {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() { continue; }

                rl.add_history_entry(input).ok();

                // Handle commands
                if input.starts_with('/') {
                    if handle_command(input, &mut llm) {
                        continue;
                    }
                }

                // Process with LLM
                match process_prompt(&llm, input, &cwd_str).await {
                    Ok(response) => println!("\n{}\n", response),
                    Err(e) => println!("\nError: {}\n", e),
                }
            }
            Err(_) => break,
        }
    }
}

fn handle_command(input: &str, llm: &mut LLM) -> bool {
    let parts: Vec<&str> = input.splitn(2, ' ').collect();
    let cmd = parts[0];

    match cmd {
        "/help" => {
            println!("\nCommands:");
            println!("  /models        - List available models");
            println!("  /model <name>  - Switch model");
            println!("  /config        - Show config path");
            println!("  /quit          - Exit\n");
        }
        "/models" => {
            println!("\nAvailable models:");
            for (id, name, provider) in MODELS {
                println!("  {} - {} ({})", id, name, provider);
            }
            println!();
        }
        "/model" => {
            if parts.len() < 2 {
                println!("Usage: /model <model_name>");
                return true;
            }
            let model = parts[1].trim();
            llm.set_model(model);
            println!("Switched to: {}", model);
        }
        "/config" => {
            let paths = Config::env_paths();
            if paths.is_empty() {
                println!("Config: .env in current dir");
            } else {
                let mut line = String::from("Config: .env in current dir");
                for path in paths {
                    line.push_str(" OR ");
                    line.push_str(&format!("{:?}", path));
                }
                println!("{}", line);
            }
        }
        "/quit" | "/exit" => {
            std::process::exit(0);
        }
        _ => {
            return false; // Not a command, process as prompt
        }
    }
    true
}

async fn process_prompt(llm: &LLM, prompt: &str, cwd: &str) -> Result<String, String> {
    let cwd_path = std::path::Path::new(cwd);
    let mut tool_results: Option<String> = None;
    let max_iterations = 10;

    // Check if prompt needs repo context (summarize, explain, understand, etc.)
    let needs_context = prompt.to_lowercase().contains("summarize")
        || prompt.to_lowercase().contains("explain")
        || prompt.to_lowercase().contains("understand")
        || prompt.to_lowercase().contains("what is this")
        || prompt.to_lowercase().contains("what does")
        || prompt.to_lowercase().contains("describe")
        || prompt.to_lowercase().contains("about this");

    // Auto-gather repo context if needed
    let repo_context = if needs_context {
        Some(gather_repo_context(cwd_path))
    } else {
        None
    };

    for _ in 0..max_iterations {
        let response = llm.chat(prompt, cwd, tool_results.as_deref(), repo_context.as_deref()).await?;

        if let Some(text) = response.response {
            return Ok(text);
        }

        if let Some(tools) = response.tools {
            if tools.is_empty() {
                return Ok("No action taken.".into());
            }

            let mut supported = Vec::new();
            let mut blocked: Vec<(ToolCall, String)> = Vec::new();
            let mut ignored = Vec::new();

            for tool in tools {
                if is_supported_action(&tool.action) {
                    if let Some(reason) = should_block_tool_for_prompt(&tool, prompt) {
                        blocked.push((tool, reason.to_string()));
                    } else {
                        supported.push(tool);
                    }
                } else {
                    ignored.push(tool);
                }
            }

            if supported.is_empty() && blocked.is_empty() && ignored.is_empty() {
                return Ok("No action taken.".into());
            }

            let mut results = Vec::new();
            for tool in &supported {
                println!("  → {} {}", tool.action, tool.path.as_deref().unwrap_or(""));
                let result = execute_tool(tool, cwd_path);
                results.push(serde_json::to_string(&result).unwrap());
            }
            for (tool, reason) in &blocked {
                let result = ToolResult {
                    action: tool.action.clone(),
                    path: tool.path.clone().unwrap_or_default(),
                    success: false,
                    result: reason.clone(),
                };
                results.push(serde_json::to_string(&result).unwrap());
            }
            for tool in &ignored {
                let result = ToolResult {
                    action: tool.action.clone(),
                    path: tool.path.clone().unwrap_or_default(),
                    success: false,
                    result: "Unsupported action".into(),
                };
                results.push(serde_json::to_string(&result).unwrap());
            }

            let results_str = results.join("\n");
            if tool_results.as_deref() == Some(results_str.as_str()) {
                return Ok("No further progress possible.".into());
            }
            tool_results = Some(results_str);
        } else {
            return Ok("No response.".into());
        }
    }

    Ok("Max iterations reached.".into())
}

fn should_block_tool_for_prompt(tool: &ToolCall, prompt: &str) -> Option<&'static str> {
    if tool.action != "create_file" && tool.action != "create_folder" {
        return None;
    }

    let path = tool.path.as_deref().unwrap_or("");
    if path.is_empty() {
        return None;
    }

    let prompt_lower = prompt.to_ascii_lowercase();
    let wants_python = contains_any(&prompt_lower, &["python", "streamlit"]);
    let wants_rust = contains_any(&prompt_lower, &["rust", "cargo"]);

    if wants_python && !wants_rust && is_rust_path(path) {
        return Some("Blocked Rust-specific file for Python/Streamlit request");
    }

    None
}

fn is_rust_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower == "cargo.toml" || lower == "cargo.lock" || lower.ends_with(".rs")
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn gather_repo_context(cwd: &std::path::Path) -> String {
    let mut context = String::new();
    
    // List files
    context.push_str("FILES:\n");
    if let Ok(entries) = std::fs::read_dir(cwd) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            let prefix = if entry.path().is_dir() { "📁 " } else { "📄 " };
            context.push_str(&format!("{}{}\n", prefix, name));
        }
    }
    
    // Read key files if they exist
    for file in ["README.md", "Cargo.toml", "package.json", "pyproject.toml", "go.mod"] {
        let path = cwd.join(file);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let truncated: String = content.chars().take(1500).collect();
                context.push_str(&format!("\n--- {} ---\n{}\n", file, truncated));
            }
        }
    }
    
    context
}
