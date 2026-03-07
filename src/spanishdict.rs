use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;

// -- Errors --

#[derive(Debug)]
pub enum SdictError {
    Fetch(reqwest::Error),
    Parse(String),
    NotFound(String),
}

impl fmt::Display for SdictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SdictError::Fetch(e) => write!(f, "Failed to fetch from SpanishDict: {e}"),
            SdictError::Parse(msg) => write!(f, "Failed to parse SpanishDict data: {msg}"),
            SdictError::NotFound(term) => write!(f, "No results for '{term}'."),
        }
    }
}

impl From<reqwest::Error> for SdictError {
    fn from(e: reqwest::Error) -> Self {
        SdictError::Fetch(e)
    }
}

// -- Data types --

#[derive(Debug, Clone)]
pub struct Term {
    pub query: String,
    pub quick_definition: Option<String>,
    pub entries: Vec<Entry>,
    pub examples: Vec<CorpusExample>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub definition: String,
    pub examples: Vec<ExampleSentence>,
}

#[derive(Debug, Clone)]
pub struct ExampleSentence {
    pub spanish: String,
    pub english: String,
}

/// An example from the corpus (separate from per-definition examples).
/// `spanish` and `english` may contain `<em>` tags for highlighting the search term.
#[derive(Debug, Clone)]
pub struct CorpusExample {
    pub spanish: String,
    pub english: String,
}

#[derive(Debug, Clone)]
pub struct FilterTag {
    pub label: String,
    pub count: usize,
}

/// Extract filter tags from corpus examples by counting the text inside `<em>` tags
/// in the English translations. Returns tags sorted by count descending.
pub fn extract_filter_tags(examples: &[CorpusExample]) -> Vec<FilterTag> {
    let re = Regex::new(r"<em>(.*?)</em>").expect("valid regex");
    let mut counts: HashMap<String, usize> = HashMap::new();

    for ex in examples {
        // Collect unique <em> texts per example to avoid double-counting
        let mut seen = std::collections::HashSet::new();
        for caps in re.captures_iter(&ex.english) {
            let text = caps[1].to_lowercase();
            if seen.insert(text.clone()) {
                *counts.entry(text).or_insert(0) += 1;
            }
        }
    }

    let mut tags: Vec<FilterTag> = counts
        .into_iter()
        .map(|(label, count)| FilterTag { label, count })
        .collect();
    tags.sort_by(|a, b| b.count.cmp(&a.count));
    tags.truncate(5);
    tags
}

/// Filter corpus examples to only those whose English text contains
/// `<em>{tag}</em>` (case-insensitive).
pub fn filter_examples(examples: &[CorpusExample], tag: &str) -> Vec<CorpusExample> {
    let tag_lower = tag.to_lowercase();
    let re = Regex::new(r"<em>(.*?)</em>").expect("valid regex");
    examples
        .iter()
        .filter(|ex| {
            re.captures_iter(&ex.english)
                .any(|caps| caps[1].to_lowercase() == tag_lower)
        })
        .cloned()
        .collect()
}

// -- Scraping --

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";

pub async fn fetch_page(client: &Client, url: &str) -> Result<String, SdictError> {
    tracing::debug!(url = %url, "fetching page");
    let response = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        tracing::warn!(url = %url, status = %status, "non-success status");
    }
    let response = response.error_for_status().map_err(SdictError::Fetch)?;
    Ok(response.text().await?)
}

pub fn extract_sd_data(html: &str) -> Result<Value, SdictError> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("script").expect("valid selector");
    let re = Regex::new(r"SD_COMPONENT_DATA = (\{.*?\});").expect("valid regex");

    for element in document.select(&selector) {
        let text = element.text().collect::<String>();
        if text.contains("SD_COMPONENT_DATA")
            && let Some(caps) = re.captures(&text)
        {
            let json_str = &caps[1];
            let value: Value = serde_json::from_str(json_str)
                .map_err(|e| SdictError::Parse(format!("Invalid JSON: {e}")))?;
            return Ok(value);
        }
    }

    tracing::warn!("no SD_COMPONENT_DATA found in HTML");
    Err(SdictError::Parse(
        "No SD_COMPONENT_DATA found in HTML".to_string(),
    ))
}

// -- Definition parsing --

pub fn parse_definitions(data: &Value) -> (Option<String>, Vec<Entry>) {
    let neodict = data
        .pointer("/sdDictionaryResultsProps/entry/neodict")
        .and_then(|v| v.as_array());

    let mut entries = Vec::new();

    if let Some(items) = neodict {
        for item in items {
            let pos_groups = item.get("posGroups").and_then(|v| v.as_array());

            if let Some(groups) = pos_groups {
                for group in groups {
                    let senses = group.get("senses").and_then(|v| v.as_array());
                    if let Some(senses) = senses {
                        for sense in senses {
                            let context = sense
                                .get("contextEn")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");

                            let translations = sense.get("translations").and_then(|v| v.as_array());
                            if let Some(translations) = translations {
                                for translation in translations {
                                    let def_text = translation
                                        .get("translation")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");

                                    let full_definition =
                                        if !context.is_empty() && !def_text.is_empty() {
                                            format!("{def_text} ({context})")
                                        } else if !def_text.is_empty() {
                                            def_text.to_string()
                                        } else {
                                            context.to_string()
                                        };

                                    let mut examples = Vec::new();
                                    if let Some(ex_array) =
                                        translation.get("examples").and_then(|v| v.as_array())
                                    {
                                        for ex in ex_array {
                                            let spanish = ex
                                                .get("textEs")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            let english = ex
                                                .get("textEn")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            if !spanish.is_empty() && !english.is_empty() {
                                                examples.push(ExampleSentence { spanish, english });
                                            }
                                        }
                                    }

                                    if !full_definition.is_empty() || !examples.is_empty() {
                                        entries.push(Entry {
                                            definition: full_definition,
                                            examples,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let quick_definition = data
        .pointer("/resultCardHeaderProps/headwordAndQuickdefsProps/quickdef1/displayText")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| entries.first().map(|e| e.definition.clone()));

    (quick_definition, entries)
}

// -- Examples section parsing --

pub fn parse_examples(data: &Value) -> Vec<CorpusExample> {
    // The examples page stores data in explorationResponseFromServerEs
    // for Spanish words (source=Spanish, target=English).
    // Each sentence has: source (Spanish with <em>), target (English with <em>), corpus, id
    let sentences = data
        .pointer("/explorationResponseFromServerEs/data/data/sentences")
        .and_then(|v| v.as_array());

    let mut examples = Vec::new();
    if let Some(sentences) = sentences {
        for sentence in sentences {
            let spanish = sentence
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let english = sentence
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !spanish.is_empty() && !english.is_empty() {
                examples.push(CorpusExample { spanish, english });
            }
        }
    }
    examples
}

// -- Public API --

pub async fn translate(client: &Client, base_url: &str, term: &str) -> Result<Term, SdictError> {
    tracing::info!(term = %term, "looking up term");
    let url = format!("{base_url}/translate/{term}");
    let html = fetch_page(client, &url).await?;
    let data = extract_sd_data(&html)?;
    let (quick_definition, entries) = parse_definitions(&data);

    if entries.is_empty() {
        return Err(SdictError::NotFound(term.to_string()));
    }

    // Fetch examples from the separate examples page (best-effort)
    let examples_url = format!("{base_url}/examples/{term}?lang=es");
    let examples = match fetch_page(client, &examples_url).await {
        Ok(examples_html) => match extract_sd_data(&examples_html) {
            Ok(examples_data) => parse_examples(&examples_data),
            Err(e) => {
                tracing::warn!(term = %term, error = %e, "failed to parse examples page");
                Vec::new()
            }
        },
        Err(e) => {
            tracing::warn!(word = %term, error = %e, "failed to fetch examples page");
            Vec::new()
        }
    };

    tracing::debug!(
        term = %term,
        definitions = entries.len(),
        examples = examples.len(),
        "lookup complete"
    );

    Ok(Term {
        query: term.to_string(),
        quick_definition,
        entries,
        examples,
    })
}
