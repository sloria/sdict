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

/// Looks up a term on SpanishDict and return its definitions and examples
pub async fn translate(
    client: &Client,
    base_url: &str,
    term: &str,
    lang_from: Option<&str>,
) -> Result<Term, SdictError> {
    tracing::info!(term, lang_from, "looking up term");
    let url = match lang_from {
        Some(lang) => format!("{base_url}/translate/{term}?langFrom={lang}"),
        None => format!("{base_url}/translate/{term}"),
    };

    // Fetch the definitions page first to determine lang_from,
    // then fetch examples with the correct ?lang= parameter.
    let html = fetch_page(client, &url).await?;
    let data = extract_sd_data(&html)?;
    let parsed = parse_definitions(&data);
    if parsed.headword_groups.is_empty() {
        return Err(SdictError::NotFound(term.to_string()));
    }

    let lang_from = parsed.lang_from.as_deref().unwrap_or("es");
    let examples_url = format!("{base_url}/examples/{term}?lang={lang_from}");

    // Fetch and parse examples
    // The examples page contains data for both language directions regardless
    // of the ?lang= parameter, so we use lang_from to select the right key.
    let examples = match fetch_page(client, &examples_url).await {
        Ok(examples_html) => match extract_sd_data(&examples_html) {
            Ok(examples_data) => parse_examples(&examples_data, lang_from),
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
        lang_from = %lang_from,
        headword_groups = parsed.headword_groups.len(),
        examples = examples.len(),
        "lookup complete"
    );

    Ok(Term {
        query: term.to_string(),
        headword: parsed.headword.unwrap_or_else(|| term.to_string()),
        quick_definitions: parsed.quick_definitions,
        headword_groups: parsed.headword_groups,
        examples,
        lang_from: lang_from.to_string(),
        has_both_langs: parsed.has_both_langs,
    })
}

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
    pub headword: String,
    pub quick_definitions: Vec<String>,
    pub headword_groups: Vec<HeadwordGroup>,
    pub examples: Vec<CorpusExample>,
    pub lang_from: String,
    pub has_both_langs: bool,
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
/// `source` is in the searched language, `target` is the translation.
/// Both may contain `<em>` tags for highlighting the search term.
/// All other HTML tags are stripped at parse time.
#[derive(Debug, Clone)]
pub struct CorpusExample {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct ParsedDefinitions {
    pub quick_definitions: Vec<String>,
    pub headword: Option<String>,
    pub headword_groups: Vec<HeadwordGroup>,
    /// Language of the search term: "es" or "en"
    pub lang_from: Option<String>,
    /// Whether the term has definitions in both language directions
    pub has_both_langs: bool,
}

#[derive(Debug, Clone)]
pub struct FilterTag {
    pub label: String,
    pub count: usize,
}

/// Extracts filter tags from corpus examples by counting the text inside `<em>` tags
/// in the target (translation) text. Returns tags sorted by count descending.
pub fn extract_filter_tags(examples: &[CorpusExample]) -> Vec<FilterTag> {
    let mut counts: HashMap<String, usize> = HashMap::new();

    for ex in examples {
        let mut seen = HashSet::new();
        for caps in EM_RE.captures_iter(&ex.target) {
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

/// Filters corpus examples to only those whose target (translation) text contains
/// `<em>{tag}</em>` (case-insensitive).
pub fn filter_examples(examples: &[CorpusExample], tag: &str) -> Vec<CorpusExample> {
    let tag_lower = tag.to_lowercase();
    examples
        .iter()
        .filter(|ex| {
            EM_RE
                .captures_iter(&ex.target)
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

/// Sanitizes HTML, allowing only `<em>` tags (used by SpanishDict to highlight
/// the search term in corpus examples). All other tags are stripped.
fn sanitize_html(s: &str) -> String {
    HTML_SANITIZER.clean(s).to_string()
}

// -- Scraping --

const USER_AGENT: &str = "sdict (+https://github.com/sloria/sdict)";

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

/// Extracts the SD_COMPONENT_DATA JSON object from the HTML, which contains
/// all the data needed to parse the definitions and examples.
pub fn extract_sd_data(html: &str) -> Result<Value, SdictError> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("script").unwrap();

    for element in document.select(&selector) {
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

/// Helper to extract an array of strings from a JSON value
/// Example: {"regionsDisplay": ["Spain", "Mexico"]} -> vec!["Spain", "Mexico"]
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

/// Parses the definitions from the SD_COMPONENT_DATA JSON.
pub fn parse_definitions(data: &Value) -> ParsedDefinitions {
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

            // posGroups: array of { pos: { nameEn }, senses: [...] }
            // senses: array of { idx, contextEn, regionsDisplay, registerLabelsDisplay, translations }
            // translations: array of { translation, examples }
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

    let quickdefs_base = "/resultCardHeaderProps/headwordAndQuickdefsProps";
    let mut quick_definitions: Vec<String> = ["quickdef1", "quickdef2"]
        .iter()
        .filter_map(|key| {
            data.pointer(&format!("{quickdefs_base}/{key}/displayText"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    if quick_definitions.is_empty()
        && let Some(fallback) = headword_groups
            .first()
            .and_then(|hw| hw.pos_groups.first())
            .and_then(|pg| pg.senses.first())
            .and_then(|s| s.translations.first())
            .map(|t| t.text.clone())
    {
        quick_definitions.push(fallback);
    }

    let headword = data
        .pointer("/resultCardHeaderProps/headwordAndQuickdefsProps/headword/displayText")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let lang_from = data
        .get("langFrom")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let has_both_langs = data
        .get("hasBothLangs")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    ParsedDefinitions {
        quick_definitions,
        headword,
        headword_groups,
        lang_from,
        has_both_langs,
    }
}

// -- Examples section parsing --

/// Parses corpus examples from the SD_COMPONENT_DATA of the examples page.
/// `lang` should be "es" or "en" to match the `?lang=` query parameter used to fetch the page.
/// The JSON key is `explorationResponseFromServerEs` for `lang=es`
/// and `explorationResponseFromServerEn` for `lang=en`.
pub fn parse_examples(data: &Value, lang: &str) -> Vec<CorpusExample> {
    let json_key = match lang {
        "en" => "/explorationResponseFromServerEn/data/data/sentences",
        _ => "/explorationResponseFromServerEs/data/data/sentences",
    };
    let sentences = data.pointer(json_key).and_then(|v| v.as_array());

    let mut examples = Vec::new();
    if let Some(sentences) = sentences {
        for sentence in sentences {
            let source = sentence
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let target = sentence
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !source.is_empty() && !target.is_empty() {
                examples.push(CorpusExample {
                    source: sanitize_html(&source),
                    target: sanitize_html(&target),
                });
            }
        }
    }
    examples
}
