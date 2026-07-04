use rig_core::client::EmbeddingsClient;
use rig_core::embeddings::{Embedding, EmbeddingError, EmbeddingModel};
use rig_core::providers::{gemini, openai};

use crate::llm_config::{EmbedConfig, EmbedProviderKind};

#[derive(Clone)]
pub enum EmbedProvider {
    OpenAI(openai::EmbeddingModel),
    Gemini(gemini::EmbeddingModel),
}

impl EmbedProvider {
    pub fn build(config: &EmbedConfig) -> Result<Self, String> {
        match config.provider {
            EmbedProviderKind::OpenAI => {
                let client = openai::Client::new(&config.api_key).map_err(|e| e.to_string())?;
                Ok(EmbedProvider::OpenAI(client.embedding_model(&config.model)))
            }
            EmbedProviderKind::Gemini => {
                let client = gemini::Client::new(&config.api_key).map_err(|e| e.to_string())?;
                Ok(EmbedProvider::Gemini(client.embedding_model(&config.model)))
            }
        }
    }
}

impl EmbeddingModel for EmbedProvider {
    const MAX_DOCUMENTS: usize = 1024;

    type Client = ();

    fn make(_client: &Self::Client, _model: impl Into<String>, _dims: Option<usize>) -> Self {
        unimplemented!("EmbedProvider instances are constructed via EmbedProvider::build")
    }

    fn ndims(&self) -> usize {
        match self {
            EmbedProvider::OpenAI(m) => m.ndims(),
            EmbedProvider::Gemini(m) => m.ndims(),
        }
    }

    async fn embed_texts(
        &self,
        texts: impl IntoIterator<Item = String> + Send,
    ) -> Result<Vec<Embedding>, EmbeddingError> {
        match self {
            EmbedProvider::OpenAI(m) => m.embed_texts(texts).await,
            EmbedProvider::Gemini(m) => m.embed_texts(texts).await,
        }
    }
}
