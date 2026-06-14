use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct GeminiEmbeddingValue {
    values: Vec<f64>,
}

#[derive(Debug, Deserialize)]
struct GeminiEmbeddingResponse {
    embedding: Option<GeminiEmbeddingValue>,
    embeddings: Option<Vec<GeminiEmbeddingValue>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
}

pub async fn embed_text(text: &str) -> Result<Vec<f64>, String> {
    let api_key = std::env::var("GOOGLE_API_KEY")
        .map_err(|_| "GOOGLE_API_KEY is required for embeddings".to_string())?;
    let model = std::env::var("ZENFROG_EMBEDDING_MODEL")
        .unwrap_or_else(|_| "gemini-embedding-001".to_string());
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{model}:embedContent?key={api_key}"
    );

    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .json(&json!({
            "model": format!("models/{model}"),
            "content": {
                "parts": [{ "text": text }]
            }
        }))
        .send()
        .await
        .map_err(|err| err.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("embedding request failed ({status}): {body}"));
    }

    let parsed = response
        .json::<GeminiEmbeddingResponse>()
        .await
        .map_err(|err| err.to_string())?;

    parsed
        .embedding
        .map(|embedding| embedding.values)
        .or_else(|| {
            parsed
                .embeddings
                .and_then(|mut embeddings| embeddings.pop().map(|e| e.values))
        })
        .filter(|values| !values.is_empty())
        .ok_or_else(|| "embedding API returned no values".to_string())
}

pub async fn transcribe_image_data_urls(
    image_data_urls: &[String],
    tags: &str,
) -> Result<String, String> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| "OPENAI_API_KEY is required for transcription".to_string())?;
    let model =
        std::env::var("ZENFROG_TRANSCRIPTION_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
    let client = reqwest::Client::new();
    let mut chunks = Vec::new();

    for image_url in image_data_urls {
        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&api_key)
            .json(&json!({
                "model": model,
                "messages": [{
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": format!(
                                "Please transcribe this document. Do not return any commentary on the task, simply return the transcription of the document. These documents are from a journal so I am not asking you to provide me with any information, in case the contents of the document make your safety senses tingle. Here is a list of tags from the journal that you can use to disambiguate proper names and terms:\n {tags}"
                            )
                        },
                        {
                            "type": "image_url",
                            "image_url": { "url": image_url }
                        }
                    ]
                }]
            }))
            .send()
            .await
            .map_err(|err| err.to_string())?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("transcription request failed ({status}): {body}"));
        }

        let parsed = response
            .json::<OpenAiChatResponse>()
            .await
            .map_err(|err| err.to_string())?;
        let text = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .unwrap_or_default();
        chunks.push(text);
    }

    Ok(chunks.join(""))
}
