use std::pin::Pin;

use futures::{Stream, StreamExt};
use rig_core::agent::{FinalResponse, MultiTurnStreamItem};
use rig_core::client::CompletionClient;
use rig_core::message::Message;
use rig_core::providers::{anthropic, gemini, openai};
use rig_core::streaming::{StreamedAssistantContent, StreamingChat};

use crate::llm_config::{ChatConfig, ChatProviderKind};

pub enum ChatProvider {
    OpenAI(openai::Client),
    Anthropic(anthropic::Client),
    Gemini(gemini::Client),
}

pub enum ChatStreamEvent {
    Delta(String),
    Done { history: Vec<Message> },
}

pub type TokenStream = Pin<Box<dyn Stream<Item = Result<ChatStreamEvent, String>> + Send>>;

impl ChatProvider {
    pub fn build(config: &ChatConfig) -> Result<Self, String> {
        match config.provider {
            ChatProviderKind::OpenAI => {
                Ok(ChatProvider::OpenAI(openai::Client::new(&config.api_key).map_err(|e| e.to_string())?))
            }
            ChatProviderKind::Anthropic => {
                Ok(ChatProvider::Anthropic(anthropic::Client::new(&config.api_key).map_err(|e| e.to_string())?))
            }
            ChatProviderKind::Gemini => {
                Ok(ChatProvider::Gemini(gemini::Client::new(&config.api_key).map_err(|e| e.to_string())?))
            }
        }
    }

    pub async fn stream_chat(
        &self,
        model: &str,
        preamble: &str,
        prompt: String,
        history: Vec<Message>,
    ) -> Result<TokenStream, String> {
        match self {
            ChatProvider::OpenAI(client) => {
                let agent = client.agent(model).preamble(preamble).build();
                let stream = agent.stream_chat(prompt, history).await;
                Ok(map_stream(stream))
            }
            ChatProvider::Anthropic(client) => {
                let agent = client.agent(model).preamble(preamble).build();
                let stream = agent.stream_chat(prompt, history).await;
                Ok(map_stream(stream))
            }
            ChatProvider::Gemini(client) => {
                let agent = client.agent(model).preamble(preamble).build();
                let stream = agent.stream_chat(prompt, history).await;
                Ok(map_stream(stream))
            }
        }
    }
}

fn map_stream<R>(
    stream: Pin<Box<dyn Stream<Item = Result<MultiTurnStreamItem<R>, rig_core::agent::StreamingError>> + Send>>,
) -> TokenStream
where
    R: Send + 'static,
{
    stream
        .map(|item| match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(text))) => {
                Ok(ChatStreamEvent::Delta(text.text))
            }
            Ok(MultiTurnStreamItem::FinalResponse(fin)) => Ok(ChatStreamEvent::Done {
                history: history_from_final(&fin),
            }),
            Ok(_) => Ok(ChatStreamEvent::Delta(String::new())),
            Err(e) => Err(e.to_string()),
        })
        .boxed()
}

fn history_from_final(fin: &FinalResponse) -> Vec<Message> {
    fin.history().map(|h| h.to_vec()).unwrap_or_default()
}
