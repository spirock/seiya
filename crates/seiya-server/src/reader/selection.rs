//! Host-controlled selection by current-run source ID, not model supplied URLs.
use super::Error;
use seiya_contracts::reader_frame::{validate_response, Request};

pub struct SourceText {
    pub id: String,
    pub url: String,
    pub text: String,
    pub text_truncated: bool,
}

/// Character budget only. The caller must still apply the real tokenizer and
/// reserve system/history/tools/output tokens; this is not context admission.
pub fn select(request: &Request, response: &[u8], ids: &[&str]) -> Result<Vec<SourceText>, Error> {
    validate_response(request, response, true).map_err(|_| Error::Format)?;
    if ids.is_empty() || ids.len() > 3 {
        return Err(Error::Policy);
    }
    let mut unique = std::collections::HashSet::new();
    if ids.iter().any(|id| !unique.insert(*id)) {
        return Err(Error::Policy);
    }
    let value: serde_json::Value = serde_json::from_slice(response).map_err(|_| Error::Format)?;
    let pages = value["results"].as_array().ok_or(Error::Format)?;
    let mut remaining = 16000usize;
    let mut selected = Vec::new();
    for id in ids {
        let (_, url) = request
            .sources()
            .find(|(source, _)| source == id)
            .ok_or(Error::Policy)?;
        let page = pages.iter().find(|p| p["id"] == *id).ok_or(Error::Policy)?;
        if page["status"] != "ok" {
            return Err(Error::NoText);
        }
        let input = page["text"].as_str().ok_or(Error::Format)?;
        let cap = remaining.min(8000);
        if cap == 0 {
            return Err(Error::Limit);
        } // Do not silently drop a selected source.
        let text: String = input.chars().take(cap).collect();
        let count = text.chars().count();
        remaining -= count;
        selected.push(SourceText {
            id: (*id).to_string(),
            url: url.to_string(),
            text_truncated: page["text_truncated"] == true || input.chars().count() > count,
            text,
        });
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    const REQUEST: &[u8] =
        include_bytes!("../../../../contracts/reader-ipc/v0.1-draft.1/request.json");
    const RESPONSE: &[u8] =
        include_bytes!("../../../../contracts/reader-ipc/v0.1-draft.1/response.json");
    #[test]
    fn ids_are_bound_to_run_and_failures_never_supply_text() {
        let request = seiya_contracts::reader_frame::parse_request(REQUEST, true).unwrap();
        assert_eq!(
            select(&request, RESPONSE, &["source-1"]).unwrap()[0].url,
            "https://example.org/article"
        );
        assert!(select(&request, RESPONSE, &["foreign"]).is_err());
        assert!(select(&request, RESPONSE, &["source-1", "source-1"]).is_err());
        let foreign = std::str::from_utf8(RESPONSE)
            .unwrap()
            .replace("run-1", "foreign");
        assert!(select(&request, foreign.as_bytes(), &["source-1"]).is_err());
    }
    #[test]
    fn source_character_clamp_is_explicit_not_token_accounting() {
        let request = seiya_contracts::reader_frame::parse_request(REQUEST, true).unwrap();
        let mut response: serde_json::Value = serde_json::from_slice(RESPONSE).unwrap();
        response["results"][0]["text"] = serde_json::json!("界".repeat(9000));
        let selected = select(
            &request,
            &serde_json::to_vec(&response).unwrap(),
            &["source-1"],
        )
        .unwrap();
        assert_eq!(selected[0].text.chars().count(), 8000);
        assert!(selected[0].text_truncated);
    }

    #[test]
    fn aggregate_cap_cannot_silently_drop_selected_ids() {
        let request=serde_json::json!({"version":"0.1-draft.1","run_id":"run-1","remaining_ms":100,
            "sources":[{"id":"a","url":"https://example.org/a"},{"id":"b","url":"https://example.org/b"},{"id":"c","url":"https://example.org/c"}]}).to_string();
        let request =
            seiya_contracts::reader_frame::parse_request(request.as_bytes(), true).unwrap();
        let pages: Vec<_>=["a","b","c"].into_iter().map(|id|serde_json::json!({"id":id,"status":"ok","text":"a".repeat(10000),"text_truncated":false,"received_bytes":10000,"decoded_bytes":10000,"header_bytes":64,"get_count":1})).collect();
        let response = serde_json::to_vec(
            &serde_json::json!({"version":"0.1-draft.1","run_id":"run-1","results":pages}),
        )
        .unwrap();
        assert_eq!(
            select(&request, &response, &["a", "b"])
                .unwrap()
                .iter()
                .map(|s| s.text.chars().count())
                .sum::<usize>(),
            16000
        );
        assert!(matches!(
            select(&request, &response, &["a", "b", "c"]),
            Err(Error::Limit)
        ));
    }
}
