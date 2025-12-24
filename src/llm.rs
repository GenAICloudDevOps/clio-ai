use crate::config::Config;
use crate::tools::{ToolCall, ToolResponse};
use reqwest::Client;
use serde_json::{json, Value};

const SYSTEM_PROMPT: &str = r#"You are an AI assistant that performs file system operations. You MUST respond with ONLY valid JSON.

CRITICAL RULES:
1. NEVER explain what you will do - just DO IT
2. ALWAYS return ONLY valid JSON, nothing else
3. NO markdown, NO code blocks, NO explanations, NO text before/after JSON
4. For file operations: {"tools": [{"action": "...", "path": "...", "content": "..."}]}
5. For questions/chat: {"response": "..."}
6. Use proper language syntax (# for Python comments, // for Rust, etc.)
7. NO HTML comments (<!-- -->) in any code files
8. Create ALL required files for complete projects; do NOT create unrelated files or scaffolding for other languages/frameworks. If a language or framework is specified, only create files for that stack.
9. Return ONLY the JSON object, nothing else
10. ONLY use the tool actions listed below. Never use actions like cd, run, exec, shell, or help.

TOOLS:
- {"action": "create_file", "path": "file.txt", "content": "file content"}
- {"action": "create_folder", "path": "folder"}
- {"action": "read_file", "path": "file.txt"}
- {"action": "delete", "path": "file.txt"}
- {"action": "list_dir", "path": "."}

EXAMPLES:

User: create hello.py with print hello
{"tools": [{"action": "create_file", "path": "hello.py", "content": "print('hello')"}]}

User: create a streamlit web app containerized with docker compose
{"tools": [{"action": "create_file", "path": "app.py", "content": "import streamlit as st\nst.title('Streamlit App')\nst.write('Hello World')"}, {"action": "create_file", "path": "requirements.txt", "content": "streamlit==1.28.0"}, {"action": "create_file", "path": "Dockerfile", "content": "FROM python:3.11-slim\nWORKDIR /app\nCOPY requirements.txt .\nRUN pip install -r requirements.txt\nCOPY app.py .\nEXPOSE 8501\nCMD [\"streamlit\", \"run\", \"app.py\"]"}, {"action": "create_file", "path": "docker-compose.yml", "content": "version: '3.8'\nservices:\n  app:\n    build: .\n    ports:\n      - '8501:8501'"}]}

User: create a folder called src with main.rs inside
{"tools": [{"action": "create_folder", "path": "src"}, {"action": "create_file", "path": "src/main.rs", "content": "fn main() {\n    println!(\"Hello\");\n}"}]}

User: what files are here?
{"tools": [{"action": "list_dir", "path": "."}]}

User: hi how are you
{"response": "Hello! I can help you create, read, and manage files. What would you like me to do?"}

Current directory: {cwd}
RESPOND WITH ONLY JSON. NO MARKDOWN. NO EXPLANATIONS."#;

pub struct LLM {
    client: Client,
    config: Config,
}

impl LLM {
    pub fn new(config: Config) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    pub fn set_model(&mut self, model: &str) {
        self.config.model = model.to_string();
        // Auto-detect provider
        if model.starts_with("gemini") {
            self.config.provider = "gemini".into();
        } else if model.starts_with("compound") || model.starts_with("meta-llama") || model.starts_with("llama-") {
            self.config.provider = "groq".into();
        } else if model.contains("llama3") || model == "llama3.2" {
            self.config.provider = "ollama".into();
        } else {
            // Default to ollama for unknown models
            self.config.provider = "ollama".into();
        }
    }

    pub async fn chat(&self, prompt: &str, cwd: &str, tool_results: Option<&str>, repo_context: Option<&str>) -> Result<ToolResponse, String> {
        let system = SYSTEM_PROMPT.replace("{cwd}", cwd);
        let user_msg = if let Some(results) = tool_results {
            format!(
                "Tool results:\n{}\n\nOriginal request: {}\n\nBased on these results, provide final response or more tool calls.",
                results,
                prompt
            )
        } else if let Some(ctx) = repo_context {
            format!("REPO CONTEXT:\n{}\n\nUSER REQUEST: {}", ctx, prompt)
        } else {
            prompt.to_string()
        };

        let response = match self.config.provider.as_str() {
            "gemini" => self.call_gemini(&system, &user_msg).await?,
            "groq" => self.call_groq(&system, &user_msg).await?,
            "ollama" => self.call_ollama(&system, &user_msg).await?,
            _ => return Err("Unknown provider".into()),
        };

        self.parse_response(&response)
    }

    async fn call_gemini(&self, system: &str, user: &str) -> Result<String, String> {
        let api_key = self.config.gemini_api_key.as_ref().ok_or("GEMINI_API_KEY not set")?;
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.config.model, api_key
        );

        let body = json!({
            "system_instruction": {"parts": [{"text": system}]},
            "contents": [{"parts": [{"text": user}]}],
            "generationConfig": {"temperature": 0.7}
        });

        let resp = self.client.post(&url).json(&body).send().await.map_err(|e| e.to_string())?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| e.to_string())?;

        if !status.is_success() {
            return Err(format!("Gemini error: HTTP {}: {}", status, text));
        }

        let json: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if let Some(message) = json.pointer("/error/message").and_then(|v| v.as_str()) {
            return Err(format!("Gemini error: {}", message));
        }

        json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("No response from Gemini: {}", json))
    }

    async fn call_groq(&self, system: &str, user: &str) -> Result<String, String> {
        let api_key = self.config.groq_api_key.as_ref().ok_or("GROQ_API_KEY not set")?;

        let body = json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "temperature": 0.7
        });

        let resp = self.client
            .post("https://api.groq.com/openai/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("Groq error: HTTP {}: {}", status, text));
        }

        let json: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if let Some(message) = json.pointer("/error/message").and_then(|v| v.as_str()) {
            return Err(format!("Groq error: {}", message));
        }

        json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("No response from Groq: {}", json))
    }

    async fn call_ollama(&self, system: &str, user: &str) -> Result<String, String> {
        let url = self.config.ollama_url.as_ref().map(|u| format!("{}/api/generate", u))
            .unwrap_or("http://localhost:11434/api/generate".into());

        let body = json!({
            "model": self.config.model,
            "prompt": user,
            "system": system,
            "stream": false
        });

        let resp = self.client.post(&url).json(&body).send().await
            .map_err(|e| format!("Ollama connection error: {}", e))?;
        
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("Ollama error: HTTP {}", status));
        }
        
        let json: Value = resp.json().await.map_err(|e| format!("Ollama parse error: {}", e))?;
        
        if let Some(response_text) = json["response"].as_str() {
            if response_text.is_empty() {
                return Err("Ollama returned empty response".into());
            }
            Ok(response_text.to_string())
        } else {
            Err(format!("Invalid Ollama response format: {:?}", json))
        }
    }

    fn parse_response(&self, text: &str) -> Result<ToolResponse, String> {
        let text = text.trim();
        
        if let Some(resp) = parse_tool_response(text) {
            return Ok(resp);
        }

        // Try to extract code blocks and create files
        let mut tools = Vec::new();
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        
        while i < lines.len() {
            let line = lines[i].trim();
            
            // Detect file patterns like **filename** or `filename`
            if let Some(filename) = extract_filename(line) {
                // Look for code block after it
                if i + 1 < lines.len() && lines[i + 1].trim().starts_with("```") {
                    let mut content = String::new();
                    i += 2; // skip filename and ```
                    while i < lines.len() && !lines[i].trim().starts_with("```") {
                        content.push_str(lines[i]);
                        content.push('\n');
                        i += 1;
                    }
                    tools.push(ToolCall {
                        action: "create_file".into(),
                        path: Some(filename),
                        content: Some(content.trim_end().to_string()),
                    });
                }
            }
            i += 1;
        }

        if !tools.is_empty() {
            return Ok(ToolResponse { tools: Some(tools), response: None });
        }

        // Fallback: treat as direct response
        Ok(ToolResponse { tools: None, response: Some(text.to_string()) })
    }
}

fn parse_tool_response(text: &str) -> Option<ToolResponse> {
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        if let Some(resp) = tool_response_from_value(value) {
            return Some(resp);
        }
    }

    for candidate in extract_json_candidates(text) {
        if let Ok(value) = serde_json::from_str::<Value>(&candidate) {
            if let Some(resp) = tool_response_from_value(value) {
                return Some(resp);
            }
        }
    }

    None
}

fn tool_response_from_value(value: Value) -> Option<ToolResponse> {
    if let Ok(resp) = serde_json::from_value::<ToolResponse>(value.clone()) {
        let has_tools = resp.tools.as_ref().map_or(false, |tools| !tools.is_empty());
        let has_response = resp.response.as_ref().map_or(false, |r| !r.trim().is_empty());
        if has_tools || has_response {
            return Some(resp);
        }
    }

    if value.is_object() {
        if value.get("action").is_some() {
            if let Ok(tool) = serde_json::from_value::<ToolCall>(value.clone()) {
                return Some(ToolResponse { tools: Some(vec![tool]), response: None });
            }
        }

        if let Some(tools_value) = value.get("tools") {
            if let Ok(tools) = serde_json::from_value::<Vec<ToolCall>>(tools_value.clone()) {
                if !tools.is_empty() {
                    return Some(ToolResponse { tools: Some(tools), response: None });
                }
            }
        }

        if let Some(response_value) = value.get("response") {
            if let Some(response_text) = response_value.as_str() {
                if !response_text.trim().is_empty() {
                    return Some(ToolResponse { tools: None, response: Some(response_text.to_string()) });
                }
            }
        }
    }

    if value.is_array() {
        if let Ok(tools) = serde_json::from_value::<Vec<ToolCall>>(value) {
            if !tools.is_empty() {
                return Some(ToolResponse { tools: Some(tools), response: None });
            }
        }
    }

    None
}

fn extract_json_candidates(text: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;

    while i < chars.len() {
        let (start_idx, ch) = chars[i];
        if ch == '{' || ch == '[' {
            let mut stack = vec![ch];
            let mut in_string = false;
            let mut escape = false;
            let mut j = i + 1;

            while j < chars.len() {
                let (_, c) = chars[j];
                if in_string {
                    if escape {
                        escape = false;
                    } else if c == '\\' {
                        escape = true;
                    } else if c == '"' {
                        in_string = false;
                    }
                } else {
                    match c {
                        '"' => in_string = true,
                        '{' | '[' => stack.push(c),
                        '}' | ']' => {
                            if let Some(open) = stack.pop() {
                                let matched = (open == '{' && c == '}') || (open == '[' && c == ']');
                                if !matched {
                                    break;
                                }
                            } else {
                                break;
                            }

                            if stack.is_empty() {
                                let end_idx = chars[j].0 + c.len_utf8();
                                candidates.push(text[start_idx..end_idx].to_string());
                                i = j;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                j += 1;
            }
        }

        i += 1;
    }

    candidates
}

fn extract_filename(line: &str) -> Option<String> {
    // Match **filename.ext** or `filename.ext`
    let line = line.trim();
    
    if line.starts_with("**") && line.ends_with("**") {
        let name = line.trim_start_matches("**").trim_end_matches("**").trim();
        if name.contains('.') || name.ends_with('/') {
            return Some(name.to_string());
        }
    }
    
    if line.starts_with('`') && line.ends_with('`') && !line.contains("```") {
        let name = line.trim_matches('`').trim();
        if name.contains('.') {
            return Some(name.to_string());
        }
    }

    // Match "filename.ext:" or "filename.ext -"
    for sep in [" (", " -", ":"] {
        if let Some(pos) = line.find(sep) {
            let name = line[..pos].trim().trim_start_matches("**").trim_end_matches("**");
            if name.contains('.') && !name.contains(' ') {
                return Some(name.to_string());
            }
        }
    }
    
    None
}
