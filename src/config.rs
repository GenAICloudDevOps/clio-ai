use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub provider: String,
    pub model: String,
    pub gemini_api_key: Option<String>,
    pub groq_api_key: Option<String>,
    pub ollama_url: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        // Try current dir first, then ~/.clio-ai/.env and ~/.ai-cli/.env
        if !dotenvy::dotenv().is_ok() {
            for path in Self::env_paths() {
                if dotenvy::from_path(&path).is_ok() {
                    break;
                }
            }
        }
        
        Self {
            provider: env::var("PROVIDER").unwrap_or("gemini".into()),
            model: env::var("MODEL").unwrap_or("gemini-3-flash-preview".into()),
            gemini_api_key: env::var("GEMINI_API_KEY").ok(),
            groq_api_key: env::var("GROQ_API_KEY").ok(),
            ollama_url: env::var("OLLAMA_URL").ok().or(Some("http://localhost:11434".into())),
        }
    }
    
    pub fn env_paths() -> Vec<PathBuf> {
        if let Some(home) = dirs::home_dir() {
            vec![
                home.join(".clio-ai").join(".env"),
                home.join(".ai-cli").join(".env"),
            ]
        } else {
            Vec::new()
        }
    }
}

pub const MODELS: &[(&str, &str, &str)] = &[
    ("gemini-3-flash-preview", "Gemini 3 Flash", "gemini"),
    ("gemini-2.5-flash-lite", "Gemini 2.5 Flash Lite", "gemini"),
    ("gemini-2.5-flash", "Gemini 2.5 Flash", "gemini"),
    ("gemini-2.5-pro", "Gemini 2.5 Pro", "gemini"),
    ("compound-beta", "Groq Compound", "groq"),
    ("meta-llama/llama-4-scout-17b-16e-instruct", "Llama 4 Scout", "groq"),
    ("llama3.2", "Llama 3.2 (Ollama)", "ollama"),
];
