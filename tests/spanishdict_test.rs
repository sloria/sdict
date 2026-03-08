use reqwest::Client;
use sdict::spanishdict::*;

fn load_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{name}")).expect("fixture file exists")
}

fn all_translations(groups: &[HeadwordGroup]) -> impl Iterator<Item = &Translation> {
    groups
        .iter()
        .flat_map(|hw| &hw.pos_groups)
        .flat_map(|pg| &pg.senses)
        .flat_map(|s| &s.translations)
}

#[test]
fn test_extract_sd_data_valid_html() {
    let html = load_fixture("comer.html");
    let data = extract_sd_data(&html).unwrap();
    assert!(data.get("sdDictionaryResultsProps").is_some());
    assert!(data.get("resultCardHeaderProps").is_some());
}

#[test]
fn test_extract_sd_data_missing_script() {
    let html = "<html><body><p>No data here</p></body></html>";
    let result = extract_sd_data(html);
    assert!(result.is_err());
    match result {
        Err(SdictError::Parse(msg)) => {
            assert!(msg.contains("No SD_COMPONENT_DATA"));
        }
        _ => panic!("Expected ParseError"),
    }
}

#[test]
fn test_extract_sd_data_malformed_json() {
    let html = r#"<html><body><script>window.SD_COMPONENT_DATA = {not valid json};</script></body></html>"#;
    let result = extract_sd_data(html);
    assert!(result.is_err());
    match result {
        Err(SdictError::Parse(msg)) => {
            assert!(msg.contains("Invalid JSON"));
        }
        _ => panic!("Expected ParseError"),
    }
}

#[test]
fn test_parse_definitions_from_fixture() {
    let html = load_fixture("comer.html");
    let data = extract_sd_data(&html).unwrap();
    let parsed = parse_definitions(&data);

    assert!(parsed.quick_definition.is_some());
    assert_eq!(parsed.headword.as_deref(), Some("comer"));
    assert!(!parsed.headword_groups.is_empty());

    // "comer" should have "to eat" as a translation
    let has_to_eat =
        all_translations(&parsed.headword_groups).any(|t| t.text.to_lowercase().contains("to eat"));
    assert!(has_to_eat, "Expected 'to eat' in definitions");

    // Should have some examples
    let total_examples: usize = all_translations(&parsed.headword_groups)
        .map(|t| t.examples.len())
        .sum();
    assert!(total_examples > 0, "Expected at least one example sentence");

    // Should have POS labels
    let first_pos = &parsed.headword_groups[0].pos_groups[0].pos_label;
    assert!(!first_pos.is_empty(), "Expected POS label");

    // Senses should have indices
    let first_sense = &parsed.headword_groups[0].pos_groups[0].senses[0];
    // Sense index should exist (u32, so always >= 0)
    let _ = first_sense.index;
}

#[test]
fn test_parse_definitions_empty_neodict() {
    let data = serde_json::json!({
        "sdDictionaryResultsProps": {
            "entry": {
                "neodict": []
            }
        }
    });
    let parsed = parse_definitions(&data);
    assert!(parsed.quick_definition.is_none());
    assert!(parsed.headword_groups.is_empty());
}

#[test]
fn test_parse_definitions_missing_fields() {
    let data = serde_json::json!({
        "sdDictionaryResultsProps": {
            "entry": {
                "neodict": [{
                    "posGroups": [{
                        "senses": [{
                            "translations": [{
                                "translation": "to eat"
                            }]
                        }]
                    }]
                }]
            }
        }
    });
    let parsed = parse_definitions(&data);
    assert_eq!(parsed.headword_groups.len(), 1);
    let translation = &parsed.headword_groups[0].pos_groups[0].senses[0].translations[0];
    assert_eq!(translation.text, "to eat");
    assert!(translation.examples.is_empty());
}

#[test]
fn test_parse_definitions_with_context() {
    let data = serde_json::json!({
        "sdDictionaryResultsProps": {
            "entry": {
                "neodict": [{
                    "posGroups": [{
                        "senses": [{
                            "contextEn": "food",
                            "translations": [{
                                "translation": "to eat",
                                "examples": [{
                                    "textEs": "Vamos a comer.",
                                    "textEn": "Let's eat."
                                }]
                            }]
                        }]
                    }]
                }]
            }
        }
    });
    let parsed = parse_definitions(&data);
    assert_eq!(parsed.headword_groups.len(), 1);
    let sense = &parsed.headword_groups[0].pos_groups[0].senses[0];
    assert_eq!(sense.context, "food");
    let translation = &sense.translations[0];
    assert_eq!(translation.text, "to eat");
    assert_eq!(translation.examples.len(), 1);
    assert_eq!(translation.examples[0].spanish, "Vamos a comer.");
    assert_eq!(translation.examples[0].english, "Let's eat.");
}

#[test]
fn test_parse_examples_from_fixture() {
    let html = load_fixture("comer_examples.html");
    let data = extract_sd_data(&html).unwrap();
    let examples = parse_examples(&data);

    assert!(!examples.is_empty());

    // First example should match what we saw in the data
    let first = &examples[0];
    assert!(first.spanish.contains("comer"));
    assert!(!first.english.is_empty());
    // Should contain <em> tags for highlighting
    assert!(first.spanish.contains("<em>"));
}

#[test]
fn test_extract_filter_tags() {
    let examples = vec![
        CorpusExample {
            spanish: "Vamos a <em>comer</em>.".to_string(),
            english: "Let's <em>eat</em>.".to_string(),
        },
        CorpusExample {
            spanish: "La hora de <em>comer</em>.".to_string(),
            english: "The <em>lunch</em> hour.".to_string(),
        },
        CorpusExample {
            spanish: "Quiero <em>comer</em> algo.".to_string(),
            english: "I want to <em>eat</em> something.".to_string(),
        },
    ];
    let tags = extract_filter_tags(&examples);
    assert_eq!(tags[0].label, "eat");
    assert_eq!(tags[0].count, 2);
    assert_eq!(tags[1].label, "lunch");
    assert_eq!(tags[1].count, 1);
}

#[test]
fn test_filter_examples() {
    let examples = vec![
        CorpusExample {
            spanish: "Vamos a <em>comer</em>.".to_string(),
            english: "Let's <em>eat</em>.".to_string(),
        },
        CorpusExample {
            spanish: "La hora de <em>comer</em>.".to_string(),
            english: "The <em>lunch</em> hour.".to_string(),
        },
    ];
    let filtered = filter_examples(&examples, "eat");
    assert_eq!(filtered.len(), 1);
    assert!(filtered[0].english.contains("eat"));

    let filtered = filter_examples(&examples, "lunch");
    assert_eq!(filtered.len(), 1);
    assert!(filtered[0].english.contains("lunch"));

    let filtered = filter_examples(&examples, "nonexistent");
    assert!(filtered.is_empty());
}

#[test]
fn test_extract_filter_tags_from_fixture() {
    let html = load_fixture("comer_examples.html");
    let data = extract_sd_data(&html).unwrap();
    let examples = parse_examples(&data);
    let tags = extract_filter_tags(&examples);

    assert!(!tags.is_empty());
    // Tags should be sorted by count descending
    for window in tags.windows(2) {
        assert!(window[0].count >= window[1].count);
    }
    // "eat" should be a common tag for "comer"
    assert!(
        tags.iter().any(|t| t.label == "eat"),
        "Expected 'eat' in filter tags"
    );
}

#[test]
fn test_parse_examples_missing_key() {
    let data = serde_json::json!({});
    let examples = parse_examples(&data);
    assert!(examples.is_empty());
}

#[tokio::test]
async fn test_translate_with_wiremock() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    let translate_html = load_fixture("comer.html");
    let examples_html = load_fixture("comer_examples.html");

    Mock::given(method("GET"))
        .and(path("/translate/comer"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&translate_html))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/examples/comer"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&examples_html))
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let result = translate(&client, &mock_server.uri(), "comer").await;
    let term = result.unwrap();

    assert_eq!(term.query, "comer");
    assert_eq!(term.headword, "comer");
    assert!(term.quick_definition.is_some());
    assert!(!term.headword_groups.is_empty());
    assert!(!term.examples.is_empty());
}

#[tokio::test]
async fn test_translate_not_found() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    // Return HTML with SD_COMPONENT_DATA but empty neodict
    let html = r#"<html><body><script>window.SD_COMPONENT_DATA = {"sdDictionaryResultsProps":{"entry":{"neodict":[]}},"resultCardHeaderProps":{}};</script></body></html>"#;

    Mock::given(method("GET"))
        .and(path("/translate/xyznotaword"))
        .respond_with(ResponseTemplate::new(200).set_body_string(html))
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let result = translate(&client, &mock_server.uri(), "xyznotaword").await;
    assert!(matches!(result, Err(SdictError::NotFound(_))));
}

#[tokio::test]
async fn test_translate_term_with_accent() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    let html = r#"<html><body><script>window.SD_COMPONENT_DATA = {"sdDictionaryResultsProps":{"entry":{"neodict":[{"posGroups":[{"senses":[{"translations":[{"translation":"common","examples":[]}]}]}]}]}},"resultCardHeaderProps":{}};</script></body></html>"#;

    Mock::given(method("GET"))
        .and(path("/translate/com%C3%BAn"))
        .respond_with(ResponseTemplate::new(200).set_body_string(html))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/examples/com%C3%BAn"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let result = translate(&client, &mock_server.uri(), "común")
        .await
        .unwrap();
    assert_eq!(result.query, "común");
    assert!(all_translations(&result.headword_groups).any(|t| t.text == "common"));
}

#[tokio::test]
async fn test_translate_term_with_spaces() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    let html = r#"<html><body><script>window.SD_COMPONENT_DATA = {"sdDictionaryResultsProps":{"entry":{"neodict":[{"posGroups":[{"senses":[{"translations":[{"translation":"good morning","examples":[]}]}]}]}]}},"resultCardHeaderProps":{}};</script></body></html>"#;

    Mock::given(method("GET"))
        .and(path("/translate/buenos%20d%C3%ADas"))
        .respond_with(ResponseTemplate::new(200).set_body_string(html))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/examples/buenos%20d%C3%ADas"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let result = translate(&client, &mock_server.uri(), "buenos días")
        .await
        .unwrap();
    assert_eq!(result.query, "buenos días");
    assert!(all_translations(&result.headword_groups).any(|t| t.text == "good morning"));
}

#[tokio::test]
async fn test_translate_fetch_error() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/translate/broken"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let result = translate(&client, &mock_server.uri(), "broken").await;
    assert!(matches!(result, Err(SdictError::Fetch(_))));
}
