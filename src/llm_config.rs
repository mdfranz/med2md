#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatProviderKind {
    OpenAI,
    Anthropic,
    Gemini,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedProviderKind {
    OpenAI,
    Gemini,
}

pub struct ChatConfig {
    pub provider: ChatProviderKind,
    pub model: String,
    pub api_key: String,
}

pub struct EmbedConfig {
    pub provider: EmbedProviderKind,
    pub model: String,
    pub api_key: String,
}

fn resolve(cli: Option<String>, env_var: &str, default: &str) -> String {
    cli.or_else(|| std::env::var(env_var).ok())
        .unwrap_or_else(|| default.to_string())
}

impl ChatProviderKind {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "openai" => Ok(ChatProviderKind::OpenAI),
            "anthropic" => Ok(ChatProviderKind::Anthropic),
            "gemini" => Ok(ChatProviderKind::Gemini),
            other => Err(format!(
                "Unknown chat provider '{}' (expected one of: openai, anthropic, gemini)",
                other
            )),
        }
    }

    fn api_key_env_var(&self) -> &'static str {
        match self {
            ChatProviderKind::OpenAI => "OPENAI_API_KEY",
            ChatProviderKind::Anthropic => "ANTHROPIC_API_KEY",
            ChatProviderKind::Gemini => "GEMINI_API_KEY",
        }
    }
}

impl EmbedProviderKind {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "openai" => Ok(EmbedProviderKind::OpenAI),
            "gemini" => Ok(EmbedProviderKind::Gemini),
            other => Err(format!(
                "Unknown embedding provider '{}' (expected one of: openai, gemini)",
                other
            )),
        }
    }

    fn api_key_env_var(&self) -> &'static str {
        match self {
            EmbedProviderKind::OpenAI => "OPENAI_API_KEY",
            EmbedProviderKind::Gemini => "GEMINI_API_KEY",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            EmbedProviderKind::OpenAI => "openai",
            EmbedProviderKind::Gemini => "gemini",
        }
    }
}

pub fn load_chat_config(
    cli_provider: Option<String>,
    cli_model: Option<String>,
) -> Result<ChatConfig, String> {
    let provider_str = resolve(cli_provider, "MED2MD_CHAT_PROVIDER", "anthropic");
    let provider = ChatProviderKind::parse(&provider_str)?;
    let model = resolve(cli_model, "MED2MD_CHAT_MODEL", "claude-sonnet-4-5");
    let env_var = provider.api_key_env_var();
    let api_key = std::env::var(env_var)
        .map_err(|_| format!("{} is not set (required for chat provider '{}')", env_var, provider_str))?;
    Ok(ChatConfig { provider, model, api_key })
}

pub fn load_embed_config(
    cli_provider: Option<String>,
    cli_model: Option<String>,
) -> Result<EmbedConfig, String> {
    let provider_str = resolve(cli_provider, "MED2MD_EMBED_PROVIDER", "openai");
    let provider = EmbedProviderKind::parse(&provider_str)?;
    let model = resolve(cli_model, "MED2MD_EMBED_MODEL", "text-embedding-3-small");
    let env_var = provider.api_key_env_var();
    let api_key = std::env::var(env_var)
        .map_err(|_| format!("{} is not set (required for embedding provider '{}')", env_var, provider_str))?;
    Ok(EmbedConfig { provider, model, api_key })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_default() {
        assert_eq!(resolve(Some("x".to_string()), "MED2MD_TEST_NONEXISTENT_VAR", "default"), "x");
    }

    #[test]
    fn default_used_when_no_cli_or_env() {
        assert_eq!(resolve(None, "MED2MD_TEST_NONEXISTENT_VAR_2", "default"), "default");
    }

    #[test]
    fn env_used_when_no_cli() {
        unsafe {
            std::env::set_var("MED2MD_TEST_ENV_VAR", "from_env");
        }
        assert_eq!(resolve(None, "MED2MD_TEST_ENV_VAR", "default"), "from_env");
        unsafe {
            std::env::remove_var("MED2MD_TEST_ENV_VAR");
        }
    }

    #[test]
    fn chat_provider_kind_parses_known_values() {
        assert_eq!(ChatProviderKind::parse("openai").unwrap(), ChatProviderKind::OpenAI);
        assert_eq!(ChatProviderKind::parse("anthropic").unwrap(), ChatProviderKind::Anthropic);
        assert_eq!(ChatProviderKind::parse("gemini").unwrap(), ChatProviderKind::Gemini);
        assert!(ChatProviderKind::parse("bogus").is_err());
    }

    #[test]
    fn embed_provider_kind_rejects_anthropic() {
        assert!(EmbedProviderKind::parse("anthropic").is_err());
    }
}
