//! A local model's details in Link's shape, whatever runs it: what a
//! sandboxed program asks of the local model route at [`PATH`], and the
//! one shape it's answered in ([`Shown`]). The models module's bridge asks
//! the runtime outside the sandbox in that runtime's own way and maps its
//! answer, field by field, to [`Shown`]; nothing of a runtime's own
//! answer passes through, so no host path, modelfile, template, license
//! or setting reaches the sandbox. A runtime Link learns to ask is one
//! mapping more, and this shape stays.

use serde::{Deserialize, Serialize};

/// Where a model's details are asked: `GET /_link/model/show?name=<name>`.
/// A `GET`, as a read is: no body for the route to read, and its one
/// argument in the query, checked as plain as a name is ([`name_of`]).
pub const PATH: &str = "/_link/model/show";

/// The longest name a show asks for.
pub const NAME_MAX: usize = 256;

/// The longest word a runtime says of a model that's kept ([`word`]).
pub const WORD_MAX: usize = 64;

/// One model's details, as Link answers them.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Shown {
    /// Link’s and the owner’s numeric model facts, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facts: Option<crate::Facts>,
    /// The model, as it was asked for.
    pub name: String,
    /// Its family or architecture (`llama`, `qwen3`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Its size in parameters, as its runtime writes it (`8.0B`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<String>,
    /// How its weights are quantized (`Q4_K_M`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization: Option<String>,
    /// The window its runtime runs it with, in tokens, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u32>,
    /// The window it was trained for, in tokens, when its runtime says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trained: Option<u32>,
    /// What it can do, each once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<Capability>,
    /// Whether the model endpoint that answered is ready, and the models
    /// it has loaded: said by an agent's model route, never by its owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<crate::api::Status>,
}

/// Something a model can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    /// It writes answers.
    Completion,
    /// It calls tools.
    Tools,
    /// It sees images.
    Vision,
    /// It makes embeddings.
    Embedding,
    /// It reasons before it answers.
    Thinking,
}

impl Capability {
    /// The capability a runtime's word for it names: Ollama's
    /// (`completion`, `tools`, `vision`, `embedding`, `thinking`) and LM
    /// Studio's (`llm`, `vlm`, `embeddings`, `tool_use`). Any other, none.
    pub fn of(word: &str) -> Option<Self> {
        Some(match word {
            "completion" | "llm" => Self::Completion,
            "tools" | "tool_use" => Self::Tools,
            "vision" | "vlm" => Self::Vision,
            "embedding" | "embeddings" => Self::Embedding,
            "thinking" => Self::Thinking,
            _ => return None,
        })
    }
}

/// Whether `name` is a model's name a show may ask for: 1 to
/// [`NAME_MAX`] letters, digits and `-_.:/`, each part between `/`s
/// more than dots (none empty, none `.`), and no `..`. Sent as it is: nothing in it needs encoding.
pub fn plain_name(name: &str) -> bool {
    (1..=NAME_MAX).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
        && !name.contains("..")
        && !name.split('/').any(|part| part.bytes().all(|b| b == b'.'))
}

/// The request's target that asks for `name`'s details, if it's a plain
/// name.
pub fn target(name: &str) -> Option<String> {
    plain_name(name).then(|| format!("{PATH}?name={name}"))
}

/// The name a request's target (`/path?query`) asks for: [`PATH`] with a
/// query of one plain `name`, and nothing else.
pub fn name_of(target: &str) -> Option<&str> {
    let (path, query) = target.split_once('?')?;
    let name = query.strip_prefix("name=")?;
    (path == PATH && plain_name(name)).then_some(name)
}

/// A word a runtime says of a model (its family, size or quantization),
/// kept only when it's plain: 1 to [`WORD_MAX`] letters, digits and
/// `-_.:`, so never a path or a sentence.
pub fn word(value: &serde_json::Value) -> Option<String> {
    let word = value.as_str()?.trim();
    ((1..=WORD_MAX).contains(&word.len())
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b)))
    .then(|| word.to_string())
}

/// A numeric parameter count, optionally with a K/M/B/T scale.
pub fn parameters(value: &serde_json::Value) -> Option<String> {
    let text = word(value)?;
    let number = text.strip_suffix(['K', 'M', 'B', 'T']).unwrap_or(&text);
    if !number
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return None;
    }
    let count = number.parse::<f64>().ok()?;
    (count.is_finite() && count > 0.0).then_some(text)
}

/// A recognized model architecture, never an arbitrary metadata word.
/// Unknown architectures are omitted until a mapping recognizes them.
pub fn family(value: &serde_json::Value) -> Option<String> {
    let family = value.as_str()?.to_ascii_lowercase();
    const KNOWN: &[&str] = &[
        "llama",
        "llama4",
        "mllama",
        "qwen",
        "qwen2",
        "qwen2moe",
        "qwen2vl",
        "qwen3",
        "qwen3moe",
        "qwen3vl",
        "gemma",
        "gemma2",
        "gemma3",
        "gemma3n",
        "mistral",
        "mixtral",
        "phi2",
        "phi3",
        "phi4",
        "falcon",
        "falcon-h1",
        "gpt2",
        "gptneox",
        "gptj",
        "gpt-oss",
        "deepseek",
        "deepseek2",
        "deepseek3",
        "deepseek4",
        "starcoder",
        "starcoder2",
        "codeshell",
        "command-r",
        "cohere",
        "cohere2",
        "bert",
        "nomic-bert",
        "nomic-bert-moe",
        "t5",
        "t5encoder",
        "bloom",
        "mamba",
        "mamba2",
        "jamba",
        "rwkv6",
        "olmo",
        "olmo2",
        "olmoe",
        "internlm2",
        "minicpm",
        "minicpm3",
        "stablelm",
        "dbrx",
        "arctic",
        "granite",
        "granitemoe",
        "exaone",
        "exaone4",
        "bailingmoe",
        "glm4",
        "glm4moe",
        "hunyuan-moe",
        "smollm3",
    ];
    KNOWN.contains(&family.as_str()).then_some(family)
}

/// A recognized weight format, normalized from runtime metadata.
pub fn quantization(value: &serde_json::Value) -> Option<String> {
    let format = value.as_str()?.to_ascii_uppercase();
    const KNOWN: &[&str] = &[
        "F64", "F32", "F16", "BF16", "FP32", "FP16", "FP8", "MXFP4", "Q4_0", "Q4_1", "Q5_0",
        "Q5_1", "Q8_0", "Q8_1", "Q8_K", "Q2_K", "Q2_K_S", "Q3_K_S", "Q3_K_M", "Q3_K_L", "Q4_K_S",
        "Q4_K_M", "Q5_K_S", "Q5_K_M", "Q6_K", "IQ1_S", "IQ1_M", "IQ2_XXS", "IQ2_XS", "IQ2_S",
        "IQ2_M", "IQ3_XXS", "IQ3_XS", "IQ3_S", "IQ3_M", "IQ4_XS", "IQ4_NL", "TQ1_0", "TQ2_0",
        "Q4_0_4_4", "Q4_0_4_8", "Q4_0_8_8",
    ];
    KNOWN.contains(&format.as_str()).then_some(format)
}

/// A window a runtime says, in tokens, kept when it's a count that fits.
pub fn tokens(value: &serde_json::Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_show_asks_for_one_plain_name() {
        assert_eq!(
            target("qwen3:8b").as_deref(),
            Some("/_link/model/show?name=qwen3:8b")
        );
        assert_eq!(
            name_of("/_link/model/show?name=library/llama3"),
            Some("library/llama3")
        );
        for refused in [
            "/_link/model/show",
            "/_link/model/show?name=",
            "/_link/model/show?name=a&x=1",
            "/_link/model/show?name=../x",
            "/_link/model/show?name=a//b",
            "/_link/model/show?name=.",
            "/_link/model/show?name=a/./b",
            "/_link/model/show?name=a/",
            "/_link/model/show?name=%2e%2e",
            "/_link/model/shown?name=a",
        ] {
            assert_eq!(name_of(refused), None, "{refused}");
        }
    }

    #[test]
    fn only_a_plain_word_is_kept() {
        assert_eq!(word(&json!("Q4_K_M")).as_deref(), Some("Q4_K_M"));
        assert_eq!(word(&json!("/home/sam/models/x.gguf")), None);
        assert_eq!(word(&json!("two words")), None);
        assert_eq!(word(&json!(7)), None);
        assert_eq!(tokens(&json!(8192)), Some(8192));
        assert_eq!(tokens(&json!(0)), None);
        assert_eq!(tokens(&json!(1u64 << 40)), None);
    }

    #[test]
    fn the_shape_is_closed() {
        let shown: Result<Shown, _> =
            serde_json::from_value(json!({"name": "a", "model_path": "/home/sam"}));
        assert!(shown.is_err());
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_metadata_word_cannot_disclose_a_host_username() {
        for name in [
            "sam",
            "root",
            "runner",
            "alice-smith",
            "workspace",
            "user123",
        ] {
            assert!(word(&json!(name)).is_some());
            assert_eq!(family(&json!(name)), None);
            assert_eq!(quantization(&json!(name)), None);
            assert_eq!(parameters(&json!(name)), None);
        }
        assert_eq!(parameters(&json!("8.0B")).as_deref(), Some("8.0B"));
        assert_eq!(family(&json!("qwen3")).as_deref(), Some("qwen3"));
        assert_eq!(quantization(&json!("q4_k_m")).as_deref(), Some("Q4_K_M"));
    }
}
