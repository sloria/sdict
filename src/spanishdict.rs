use ammonia::Builder;
use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::LazyLock;

static EM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<em>(.*?)</em>").unwrap());
static SD_DATA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"SD_COMPONENT_DATA = (\{.*?\});").unwrap());
static SCRIPT_SELECTOR: LazyLock<Selector> = LazyLock::new(|| Selector::parse("script").unwrap());

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
    pub headword_groups: Vec<HeadwordGroup>,
    pub examples: Vec<CorpusExample>,
}

#[derive(Debug, Clone)]
pub struct HeadwordGroup {
    pub subheadword: String,
    pub pos_groups: Vec<PosGroup>,
}

#[derive(Debug, Clone)]
pub struct PosGroup {
    pub pos_label: String,
    pub senses: Vec<Sense>,
}

#[derive(Debug, Clone)]
pub struct Sense {
    pub index: u32,
    pub context: String,
    pub regions: Vec<String>,
    pub register_labels: Vec<String>,
    pub translations: Vec<Translation>,
}

#[derive(Debug, Clone)]
pub struct Translation {
    pub text: String,
    pub examples: Vec<ExampleSentence>,
}

#[derive(Debug, Clone)]
pub struct ExampleSentence {
    pub spanish: String,
    pub english: String,
}

/// An example from the corpus (separate from per-definition examples).
/// `spanish` and `english` may contain `<em>` tags for highlighting the search term.
/// All other HTML tags are stripped at parse time.
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
    let mut counts: HashMap<String, usize> = HashMap::new();

    for ex in examples {
        let mut seen = HashSet::new();
        for caps in EM_RE.captures_iter(&ex.english) {
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
    examples
        .iter()
        .filter(|ex| {
            EM_RE
                .captures_iter(&ex.english)
                .any(|caps| caps[1].to_lowercase() == tag_lower)
        })
        .cloned()
        .collect()
}

// Allow only <em> tags — SpanishDict uses these to highlight the search term
// in corpus examples. Rendered with |safe in templates after sanitization.
static HTML_SANITIZER: LazyLock<Builder<'static>> = LazyLock::new(|| {
    let mut builder = Builder::new();
    builder.tags(HashSet::from(["em"]));
    builder
});

/// Sanitize HTML, allowing only `<em>` tags (used by SpanishDict to highlight
/// the search term in corpus examples). All other tags are stripped.
fn sanitize_html(s: &str) -> String {
    HTML_SANITIZER.clean(s).to_string()
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

    for element in document.select(&SCRIPT_SELECTOR) {
        let text = element.text().collect::<String>();
        if text.contains("SD_COMPONENT_DATA")
            && let Some(caps) = SD_DATA_RE.captures(&text)
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

fn extract_string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_definitions(data: &Value) -> (Option<String>, Vec<HeadwordGroup>) {
    let neodict = data
        .pointer("/sdDictionaryResultsProps/entry/neodict")
        .and_then(|v| v.as_array());

    let mut headword_groups = Vec::new();

    if let Some(items) = neodict {
        for item in items {
            let subheadword = item
                .get("subheadword")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let mut pos_groups = Vec::new();

            if let Some(groups) = item.get("posGroups").and_then(|v| v.as_array()) {
                for group in groups {
                    let pos_label = group
                        .pointer("/pos/nameEn")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let mut senses = Vec::new();

                    if let Some(sense_array) = group.get("senses").and_then(|v| v.as_array()) {
                        for sense_val in sense_array {
                            let index =
                                sense_val.get("idx").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

                            let context = sense_val
                                .get("contextEn")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            let regions = extract_string_array(sense_val, "regionsDisplay");
                            let register_labels =
                                extract_string_array(sense_val, "registerLabelsDisplay");

                            let mut translations = Vec::new();

                            if let Some(trans_array) =
                                sense_val.get("translations").and_then(|v| v.as_array())
                            {
                                for trans_val in trans_array {
                                    let text = trans_val
                                        .get("translation")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();

                                    let mut examples = Vec::new();
                                    if let Some(ex_array) =
                                        trans_val.get("examples").and_then(|v| v.as_array())
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

                                    if !text.is_empty() || !examples.is_empty() {
                                        translations.push(Translation { text, examples });
                                    }
                                }
                            }

                            if !translations.is_empty() {
                                senses.push(Sense {
                                    index,
                                    context,
                                    regions,
                                    register_labels,
                                    translations,
                                });
                            }
                        }
                    }

                    if !senses.is_empty() {
                        pos_groups.push(PosGroup { pos_label, senses });
                    }
                }
            }

            if !pos_groups.is_empty() {
                headword_groups.push(HeadwordGroup {
                    subheadword,
                    pos_groups,
                });
            }
        }
    }

    let quick_definition = data
        .pointer("/resultCardHeaderProps/headwordAndQuickdefsProps/quickdef1/displayText")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            headword_groups
                .first()
                .and_then(|hw| hw.pos_groups.first())
                .and_then(|pg| pg.senses.first())
                .and_then(|s| s.translations.first())
                .map(|t| t.text.clone())
        });

    (quick_definition, headword_groups)
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
                examples.push(CorpusExample {
                    spanish: sanitize_html(&spanish),
                    english: sanitize_html(&english),
                });
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
    let (quick_definition, headword_groups) = parse_definitions(&data);

    if headword_groups.is_empty() {
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
        headword_groups = headword_groups.len(),
        examples = examples.len(),
        "lookup complete"
    );

    Ok(Term {
        query: term.to_string(),
        quick_definition,
        headword_groups,
        examples,
    })
}
