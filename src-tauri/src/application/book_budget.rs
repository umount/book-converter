//! Plan translation requests from the complete prompt and the provider's token budgets.
use super::book::Segment;
use crate::{
    ai::{ProviderProfile, Request},
    app::contracts::AppError,
};

// Without a provider tokenizer, UTF-8 bytes give a conservative input estimate.
// Output is language-dependent: reserve three tokens per source character plus JSON.
// These are estimates, not a guarantee against a provider truncating its answer.
fn fits(
    profile: &ProviderProfile,
    segments: &[Segment],
    request: &impl Fn(&[Segment]) -> Result<Request, AppError>,
) -> Result<bool, AppError> {
    let Request::Structured { system, user } = request(segments)? else {
        return Err(AppError::invalid("translationBudget"));
    };
    let input = system.len() as u64 + user.len() as u64 + 256;
    let output = 128
        + segments
            .iter()
            .map(|s| s.text.chars().count() as u64 * 3 + s.id.len() as u64 + 128)
            .sum::<u64>();
    Ok(output <= u64::from(profile.max_output_tokens) * 9 / 10
        && input + u64::from(profile.max_output_tokens)
            <= u64::from(profile.context_window_tokens) * 9 / 10)
}

/// Try the entire chapter first. Split only oversized blocks, without dropping or
/// repeating any source bytes. Prefer paragraph, then sentence boundaries.
pub(super) fn plan(
    profile: &ProviderProfile,
    segments: &[Segment],
    request: impl Fn(&[Segment]) -> Result<Request, AppError>,
) -> Result<Vec<Vec<Segment>>, AppError> {
    if fits(profile, segments, &request)? {
        return Ok(vec![segments.to_vec()]);
    }
    let mut parts = Vec::new();
    for segment in segments {
        if fits(profile, std::slice::from_ref(segment), &request)? {
            parts.push(segment.clone());
            continue;
        }
        let mut rest = segment.text.as_str();
        let mut index = 0;
        while !rest.is_empty() {
            let id = format!("{}:part{index}", segment.id);
            let boundaries: Vec<_> = rest
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(rest.len()))
                .collect();
            let (mut low, mut high) = (0, boundaries.len() - 1);
            while low < high {
                let mid = (low + high + 1) / 2;
                let candidate = Segment {
                    id: id.clone(),
                    text: rest[..boundaries[mid]].into(),
                };
                if fits(profile, &[candidate], &request)? {
                    low = mid;
                } else {
                    high = mid - 1;
                }
            }
            if low == 0 {
                return Err(AppError::invalid("translationBudget"));
            }
            let mut end = boundaries[low];
            if end < rest.len() {
                let prefix = &rest[..end];
                if let Some(at) = prefix.rfind('\n') {
                    end = at + 1;
                } else if let Some((at, c)) = prefix
                    .char_indices()
                    .rev()
                    .find(|(_, c)| matches!(c, '。' | '！' | '？' | '.' | '!' | '?'))
                {
                    end = at + c.len_utf8();
                }
            }
            let part = Segment {
                id,
                text: rest[..end].into(),
            };
            // The glossary is selected from each candidate. Recheck after changing
            // a boundary; future prompt changes must not bypass the budget check.
            if !fits(profile, std::slice::from_ref(&part), &request)? {
                return Err(AppError::invalid("translationBudget"));
            }
            parts.push(part);
            rest = &rest[end..];
            index += 1;
        }
        if segment.text.is_empty() {
            return Err(AppError::invalid("translationBudget"));
        }
    }
    let mut batches = Vec::new();
    let mut batch = Vec::new();
    for part in parts {
        batch.push(part);
        if !fits(profile, &batch, &request)? {
            let last = batch.pop().unwrap();
            if batch.is_empty() {
                return Err(AppError::invalid("translationBudget"));
            }
            batches.push(std::mem::take(&mut batch));
            batch.push(last);
        }
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(context: u32, output: u32) -> ProviderProfile {
        ProviderProfile {
            id: "test".into(),
            base_url: "https://unused.test".into(),
            model: "test".into(),
            temperature: 0.0,
            context_window_tokens: context,
            max_output_tokens: output,
            timeout_seconds: 1,
            network_retries: 0,
        }
    }
    fn request(parts: &[Segment]) -> Result<Request, AppError> {
        Ok(Request::Structured {
            system: "Translate".into(),
            user: serde_json::json!({"segments":parts}).to_string(),
        })
    }
    #[test]
    fn normal_chapters_stay_whole_and_many_blocks_share_one_request() {
        for len in [3527, 3982] {
            let segments = vec![
                Segment {
                    id: "title:0".into(),
                    text: "第十九章".into(),
                },
                Segment {
                    id: "body:0".into(),
                    text: "文".repeat(len),
                },
            ];
            let batches = plan(&profile(1_000_000, 384_000), &segments, request).unwrap();
            assert_eq!(batches.len(), 1);
            assert_eq!(batches[0].len(), 2);
            assert_eq!(batches[0][1].text, segments[1].text);
        }
        let segments: Vec<_> = (0..12)
            .map(|n| Segment {
                id: n.to_string(),
                text: "Paragraph".into(),
            })
            .collect();
        assert_eq!(
            plan(&profile(32768, 4096), &segments, request).unwrap()[0].len(),
            12
        );
    }
    #[test]
    fn output_limit_splits_on_paragraphs_without_loss_or_overlap() {
        let text = "第一段文字。\n第二段文字！\n\n".repeat(100);
        let p = profile(100_000, 1000);
        let batches = plan(
            &p,
            &[Segment {
                id: "body:0".into(),
                text: text.clone(),
            }],
            request,
        )
        .unwrap();
        assert!(batches.len() > 1);
        assert_eq!(
            batches
                .iter()
                .flatten()
                .map(|s| s.text.as_str())
                .collect::<String>(),
            text
        );
        for batch in &batches {
            assert!(fits(&p, batch, &request).unwrap());
        }
        let parts: Vec<_> = batches.iter().flatten().collect();
        for part in &parts[..parts.len() - 1] {
            assert!(part.text.ends_with('\n'));
        }
    }
    #[test]
    fn context_budget_counts_instructions_history_reference_and_glossary() {
        let p = profile(8000, 4000);
        let segment = Segment {
            id: "body".into(),
            text: "文".repeat(700),
        };
        assert_eq!(plan(&p, &[segment.clone()], request).unwrap().len(), 1);
        let with_context = |parts: &[Segment]| {
            Ok(Request::Structured {
                system: "上下文".repeat(240),
                user: serde_json::json!({"segments":parts}).to_string(),
            })
        };
        let batches = plan(&p, &[segment.clone()], with_context).unwrap();
        assert!(batches.len() > 1);
        for batch in &batches {
            assert!(fits(&p, batch, &with_context).unwrap());
        }
        assert_eq!(
            batches
                .iter()
                .flatten()
                .map(|s| s.text.as_str())
                .collect::<String>(),
            segment.text
        );
        let too_big = |_: &[Segment]| {
            Ok(Request::Structured {
                system: "x".repeat(8000),
                user: "{}".into(),
            })
        };
        assert!(plan(&p, &[segment], too_big).is_err());
    }
    #[test]
    fn sentence_and_unicode_fallbacks_preserve_every_byte() {
        for text in ["一句话。".repeat(100), "🙂".repeat(100)] {
            let batches = plan(
                &profile(32000, 500),
                &[Segment {
                    id: "body".into(),
                    text: text.clone(),
                }],
                request,
            )
            .unwrap();
            assert_eq!(
                batches
                    .iter()
                    .flatten()
                    .map(|s| s.text.as_str())
                    .collect::<String>(),
                text
            );
        }
    }
}
