use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub action: String,
    pub path: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolResponse {
    pub tools: Option<Vec<ToolCall>>,
    pub response: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ToolResult {
    pub action: String,
    pub path: String,
    pub success: bool,
    pub result: String,
}

pub fn is_supported_action(action: &str) -> bool {
    matches!(
        action,
        "read_file" | "create_file" | "create_folder" | "delete" | "list_dir"
    )
}

pub fn execute_tool(tool: &ToolCall, cwd: &Path) -> ToolResult {
    let path_str = tool.path.clone().unwrap_or(".".into());
    let full_path = cwd.join(&path_str);
    
    // Security: ensure path is within cwd
    let canonical_cwd = cwd.canonicalize().unwrap_or(cwd.to_path_buf());
    let canonical_path = full_path.canonicalize().unwrap_or(full_path.clone());
    
    if !canonical_path.starts_with(&canonical_cwd) && tool.action != "list_dir" {
        return ToolResult {
            action: tool.action.clone(),
            path: path_str,
            success: false,
            result: "Access denied: path outside current directory".into(),
        };
    }

    match tool.action.as_str() {
        "read_file" => {
            match fs::read_to_string(&full_path) {
                Ok(content) => ToolResult {
                    action: "read_file".into(),
                    path: path_str,
                    success: true,
                    result: content,
                },
                Err(e) => ToolResult {
                    action: "read_file".into(),
                    path: path_str,
                    success: false,
                    result: e.to_string(),
                },
            }
        }
        "create_file" => {
            let content = tool.content.clone().unwrap_or_default();
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent).ok();
            }
            match fs::write(&full_path, &content) {
                Ok(_) => ToolResult {
                    action: "create_file".into(),
                    path: path_str,
                    success: true,
                    result: format!("Created file with {} bytes", content.len()),
                },
                Err(e) => ToolResult {
                    action: "create_file".into(),
                    path: path_str,
                    success: false,
                    result: e.to_string(),
                },
            }
        }
        "create_folder" => {
            match fs::create_dir_all(&full_path) {
                Ok(_) => ToolResult {
                    action: "create_folder".into(),
                    path: path_str,
                    success: true,
                    result: "Folder created".into(),
                },
                Err(e) => ToolResult {
                    action: "create_folder".into(),
                    path: path_str,
                    success: false,
                    result: e.to_string(),
                },
            }
        }
        "delete" => {
            let result = if full_path.is_dir() {
                fs::remove_dir_all(&full_path)
            } else {
                fs::remove_file(&full_path)
            };
            match result {
                Ok(_) => ToolResult {
                    action: "delete".into(),
                    path: path_str,
                    success: true,
                    result: "Deleted".into(),
                },
                Err(e) => ToolResult {
                    action: "delete".into(),
                    path: path_str,
                    success: false,
                    result: e.to_string(),
                },
            }
        }
        "list_dir" => {
            match fs::read_dir(&full_path) {
                Ok(entries) => {
                    let files: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .map(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            if e.path().is_dir() { format!("{}/", name) } else { name }
                        })
                        .collect();
                    ToolResult {
                        action: "list_dir".into(),
                        path: path_str,
                        success: true,
                        result: files.join("\n"),
                    }
                }
                Err(e) => ToolResult {
                    action: "list_dir".into(),
                    path: path_str,
                    success: false,
                    result: e.to_string(),
                },
            }
        }
        _ => ToolResult {
            action: tool.action.clone(),
            path: path_str,
            success: false,
            result: "Unknown action".into(),
        },
    }
}
