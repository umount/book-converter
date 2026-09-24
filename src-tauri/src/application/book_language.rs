//! Bounded, line-scoped language repair for structural translations.
use crate::{
    ai::{Provider, Request},
    textutil,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    n: usize,
    text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    lines: Vec<Line>,
}

/// A failed repair never discards a successful translation. Remaining issues are
/// scanned by the reader. Source/image blocks and correctly translated lines stay intact.
pub async fn repair(
    provider: &dyn Provider,
    target: &str,
    source: &str,
    title: &mut String,
    blocks: &mut [(String, String)],
    glossary: &str,
) {
    let Some(script) = textutil::expected_script(target) else {
        return;
    };
    let mut lines = vec![title.clone()];
    let mut ranges = Vec::new();
    for (_, text) in blocks.iter() {
        let start = lines.len();
        lines.extend(text.split('\n').map(str::to_owned));
        ranges.push(start..lines.len());
    }
    let issues =
        |text: &str| textutil::foreign_fragments(text, script, source, textutil::MIN_FOREIGN_RUN);
    for _ in 0..2 {
        let targets: Vec<_> = lines
            .iter()
            .enumerate()
            .filter(|(_, text)| !issues(text).is_empty())
            .map(|(n, text)| Line {
                n,
                text: text.clone(),
            })
            .collect();
        if targets.is_empty() {
            break;
        }
        let mut changed = false;
        let mut start = 0;
        while start < targets.len() {
            let mut end = start;
            let mut chars = 0;
            while end < targets.len() {
                let size = targets[end].text.chars().count();
                if end > start && chars + size > 6000 {
                    break;
                }
                chars += size;
                end += 1;
            }
            let batch = &targets[start..end];
            start = end;
            let system=format!("Repair only foreign-language fragments in these {target} translation lines. Use the established glossary: {glossary}. Transliterate personal names appropriately for {target}; translate other terms faithfully. Preserve meaning, punctuation, whitespace and all other wording. Never summarize, merge or split lines. Return only JSON {{\"lines\":[{{\"n\":0,\"text\":\"corrected line\"}}]}} using only supplied line numbers.");
            let response = provider
                .complete(Request::Structured {
                    system,
                    user: serde_json::json!({"lines":batch}).to_string(),
                })
                .await;
            let Ok(response) = response else { break };
            if response.finish_reason != "stop" {
                continue;
            }
            let Ok(reply) = serde_json::from_str::<Reply>(&response.text) else {
                continue;
            };
            let expected: HashSet<_> = batch.iter().map(|l| l.n).collect();
            let mut seen = HashSet::new();
            if reply
                .lines
                .iter()
                .any(|l| !expected.contains(&l.n) || !seen.insert(l.n))
            {
                continue;
            }
            for fixed in reply.lines {
                let original = &lines[fixed.n];
                if fixed.text.trim().is_empty()
                    || fixed.text.contains(['\n', '\r'])
                    || fixed.text.trim().chars().count() * 2 < original.trim().chars().count()
                    || issues(&fixed.text).len() >= issues(original).len()
                {
                    continue;
                }
                let leading = &original[..original.len() - original.trim_start().len()];
                let trailing = &original[original.trim_end().len()..];
                lines[fixed.n] = format!("{leading}{}{trailing}", fixed.text.trim());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    *title = lines[0].clone();
    for ((_, text), range) in blocks.iter_mut().zip(ranges) {
        *text = lines[range].join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ai::{Completion, ProviderProfile, Usage},
        app::contracts::AppError,
    };
    use std::{collections::VecDeque, future::Future, pin::Pin, sync::Mutex};
    struct Fake {
        profile: ProviderProfile,
        replies: Mutex<VecDeque<Result<String, AppError>>>,
        sent: Mutex<Vec<serde_json::Value>>,
    }
    impl Fake {
        fn new(replies: Vec<&str>) -> Self {
            Self {
                profile: ProviderProfile {
                    id: "test".into(),
                    base_url: "https://unused.test".into(),
                    model: "test".into(),
                    temperature: 0.,
                    max_output_tokens: 1000,
                    timeout_seconds: 1,
                    network_retries: 0,
                },
                replies: Mutex::new(replies.into_iter().map(|s| Ok(s.into())).collect()),
                sent: Mutex::new(vec![]),
            }
        }
    }
    impl Provider for Fake {
        fn profile(&self) -> &ProviderProfile {
            &self.profile
        }
        fn complete(
            &self,
            request: Request,
        ) -> Pin<Box<dyn Future<Output = Result<Completion, AppError>> + Send + '_>> {
            let Request::Structured { user, system } = request else {
                panic!("structured repair")
            };
            let mut payload: serde_json::Value = serde_json::from_str(&user).unwrap();
            payload["system"] = system.into();
            self.sent.lock().unwrap().push(payload);
            Box::pin(async {
                Ok(Completion {
                    text: self
                        .replies
                        .lock()
                        .unwrap()
                        .pop_front()
                        .expect("bounded requests")?,
                    finish_reason: "stop".into(),
                    usage: Usage::default(),
                    tool_calls: vec![],
                })
            })
        }
    }
    #[tokio::test]
    async fn repairs_only_affected_lines_and_preserves_block_layout() {
        let fake = Fake::new(vec![
            r#"{"lines":[{"n":0,"text":"Ван Линь"},{"n":3,"text":"Он увидел Ван Линя."}]}"#,
        ]);
        let mut title = "王林".into();
        let mut blocks = vec![
            ("block-a".into(), "Чистая строка.\n\nОн увидел 王林.".into()),
            ("block-b".into(), "  Без изменений.  \n".into()),
        ];
        repair(&fake, "ru", "王林", &mut title, &mut blocks, "[]").await;
        assert_eq!(title, "Ван Линь");
        assert_eq!(
            blocks[0],
            (
                "block-a".into(),
                "Чистая строка.\n\nОн увидел Ван Линя.".into()
            )
        );
        assert_eq!(blocks[1], ("block-b".into(), "  Без изменений.  \n".into()));
        let sent = fake.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["lines"].as_array().unwrap().len(), 2);
        assert_eq!(sent[0]["lines"][1]["n"], 3);
    }
    #[tokio::test]
    async fn title_only_repair_includes_glossary_without_chapter_body() {
        let fake = Fake::new(vec![r#"{"lines":[{"n":0,"text":"Ван Линь"}]}"#]);
        let mut title = "  王林  ".into();
        let mut blocks = vec![("a".into(), "Тело главы не отправляется.".into())];
        let original = blocks.clone();
        repair(
            &fake,
            "ru",
            "王林",
            &mut title,
            &mut blocks,
            r#"[{"source":"王林","target":"Ван Линь","pinned":true}]"#,
        )
        .await;
        assert_eq!(title, "  Ван Линь  ");
        assert_eq!(blocks, original);
        let sent = fake.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["lines"].as_array().unwrap().len(), 1);
        assert_eq!(sent[0]["lines"][0]["n"], 0);
        assert!(sent[0]["system"].as_str().unwrap().contains("Ван Линь"));
        assert!(!sent[0].to_string().contains("Тело главы"));
    }

    #[tokio::test]
    async fn malformed_or_unsolicited_edits_never_replace_translation() {
        for reply in [
            "bad",
            r#"{"lines":[{"n":0,"text":"Ненужное изменение"},{"n":1,"text":"Ван Линь пришёл."}]}"#,
            r#"{"lines":[{"n":1,"text":"Ван Линь"},{"n":1,"text":"Дубликат"}]}"#,
            r#"{"lines":[{"n":1,"text":""}]}"#,
            r#"{"lines":[{"n":1,"text":"Ван\nЛинь пришёл."}]}"#,
        ] {
            let fake = Fake::new(vec![reply]);
            let mut title = "Глава".into();
            let original = vec![("a".into(), "王林 пришёл.".into())];
            let mut blocks = original.clone();
            repair(&fake, "ru", "", &mut title, &mut blocks, "[]").await;
            assert_eq!(blocks, original);
            assert_eq!(title, "Глава");
            assert_eq!(fake.sent.lock().unwrap().len(), 1);
        }
    }
    #[tokio::test]
    async fn two_passes_and_network_failure_keep_unresolved_text() {
        let fake = Fake::new(vec![
            r#"{"lines":[{"n":1,"text":"Ван Линь увидел 李雷 и 韩立."}]}"#,
            r#"{"lines":[{"n":1,"text":"Ван Линь увидел Ли Лэя и 韩立."}]}"#,
        ]);
        let mut title = "Глава".into();
        let mut blocks = vec![("a".into(), "王林 увидел 李雷 и 韩立.".into())];
        repair(&fake, "ru", "", &mut title, &mut blocks, "[]").await;
        assert_eq!(fake.sent.lock().unwrap().len(), 2);
        assert!(blocks[0].1.contains("韩立"));
        let failed = Fake::new(vec![]);
        failed
            .replies
            .lock()
            .unwrap()
            .push_back(Err(AppError::invalid("offline")));
        let before = blocks.clone();
        repair(&failed, "ru", "", &mut title, &mut blocks, "[]").await;
        assert_eq!(blocks, before);
    }
    #[tokio::test]
    async fn clean_or_unsupported_languages_make_no_requests() {
        let fake = Fake::new(vec![]);
        let mut title = "Глава".into();
        let mut blocks = vec![("a".into(), "В тексте слово OpenAI.".into())];
        repair(&fake, "ru", "OpenAI", &mut title, &mut blocks, "[]").await;
        repair(&fake, "ja", "", &mut title, &mut blocks, "[]").await;
        assert!(fake.sent.lock().unwrap().is_empty());
    }
}
