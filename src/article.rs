//! Deep-article evidence contracts. Scheduling and result authority remain in TaskLoop.
use async_trait::async_trait;
mod archive;
pub mod discovery;
pub mod revise;
pub use archive::ArchivedBody;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use taskloop::*;
use tokio_util::sync::CancellationToken;

pub const CORPUS_BYTES: usize = 1_048_576;
pub const ARCHIVE_BYTES: usize = 2_621_440;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub source_id: String,
    pub origin: String,
    pub fact: Value,
    pub paragraphs: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corpus {
    pub version: u32,
    pub round: u8,
    pub sources: Vec<Source>,
    pub searches: Vec<Value>,
    pub limitations: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Citation {
    pub artifact_id: String,
    pub sha256: String,
    pub source_id: String,
    pub paragraph: usize,
    pub quote: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub answer_status: String,
    pub conclusion: String,
    pub citations: Vec<Citation>,
    pub limitations: Vec<String>,
}
pub fn fault(message: impl Into<String>) -> Error {
    Error::new("research_invalid", "/article", message)
}

/// The caller must obey this sentence. An `Err` loses the text on the host
/// tool path, so the sentence is a successful tool value instead.
pub fn model_refusal(message: impl Into<String>) -> Value {
    json!({
        "ok": false,
        "code": "rejected",
        "message": message.into(),
    })
}
fn corpus(artifact: &ArtifactData) -> Result<Corpus> {
    let data: Corpus = serde_json::from_value(parse_unique(&artifact.bytes)?)?;
    if data.version != 1 || !(1..=2).contains(&data.round) {
        return Err(fault("资料包版本或轮次不受支持"));
    }
    Ok(data)
}
pub fn source_view(artifact: &ArtifactData, source_id: &str) -> Result<Source> {
    corpus(artifact)?
        .sources
        .into_iter()
        .find(|s| s.source_id == source_id)
        .ok_or_else(|| Error::new("not_found", "/source_id", "此资料包没有该来源"))
}
pub fn verify_citation(c: &Citation, accepted: &[ArtifactData]) -> Result<String> {
    let artifact = accepted
        .iter()
        .find(|a| a.reference.id == c.artifact_id)
        .ok_or_else(|| fault("引用资料包未被授权"))?;
    if artifact.reference.sha256 != c.sha256 || crate::domain::hash(&artifact.bytes) != c.sha256 {
        return Err(fault("引用资料包版本不匹配"));
    }
    let source = source_view(artifact, &c.source_id)?;
    if source.fact["usable"] != true
        || c.quote.trim().is_empty()
        || c.quote.chars().count() > 200
        || !source
            .paragraphs
            .get(c.paragraph)
            .is_some_and(|p| p.contains(&c.quote))
        || source.fact["normalized_sha256"]
            != crate::domain::hash(&serde_json::to_vec(&source.paragraphs)?)
    {
        return Err(fault("引用不是已保存来源版本的精确短引文"));
    }
    source.fact["final_url"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| fault("引用来源缺少实际获取网址"))
}
pub fn check_analysis(analysis: &Value, accepted: &[ArtifactData]) -> Result<()> {
    let evidence = analysis["evidence"]
        .as_array()
        .ok_or_else(|| fault("缺少分析证据"))?;
    let mut ids = BTreeSet::new();
    for item in evidence {
        let id = item["id"].as_str().ok_or_else(|| fault("缺少证据身份"))?;
        if !ids.insert(id) {
            return Err(fault("分析证据身份重复"));
        }
        let mut c = item.clone();
        c.as_object_mut()
            .ok_or_else(|| fault("证据必须为对象"))?
            .remove("id");
        verify_citation(&serde_json::from_value(c)?, accepted)?;
    }
    for judgment in analysis["judgments"]
        .as_array()
        .ok_or_else(|| fault("缺少分析判断"))?
    {
        let support = judgment["support"]
            .as_array()
            .ok_or_else(|| fault("缺少支持关系"))?;
        let counter = judgment["counter"]
            .as_array()
            .ok_or_else(|| fault("缺少反驳关系"))?;
        if support.is_empty()
            || support
                .iter()
                .chain(counter)
                .any(|v| v.as_str().is_none_or(|id| !ids.contains(id)))
        {
            return Err(fault("分析判断关联悬空或缺少支持证据"));
        }
    }
    for coverage in analysis["coverage"]
        .as_array()
        .ok_or_else(|| fault("缺少问题覆盖关系"))?
    {
        if coverage["evidence_ids"]
            .as_array()
            .ok_or_else(|| fault("缺少覆盖证据关系"))?
            .iter()
            .any(|v| v.as_str().is_none_or(|id| !ids.contains(id)))
        {
            return Err(fault("问题覆盖关联悬空证据"));
        }
    }
    Ok(())
}
/// The saved paragraph states each sentence that carries this marker.
/// A short quote inside the paragraph is not enough when the sentence says something else.
/// A marker after the sentence end belongs to that sentence when it stays in the same block. A marker after a blank line does not.
/// List marks, links, emphasis, and table pipes are article syntax. A table row is compared through the cells that carry a marker.
/// The citation marker may sit inside the sentence. A different in-sentence marker, such as [12], stays.
/// The remaining sentence must equal one whole sentence of the paragraph. A fragment inside a longer sentence does not pass.
/// A Markdown soft wrap stays inside that sentence. A blank line, heading, or new list item starts another.
/// A period ends an English sentence before a citation marker, at the end of the text, or when the next sentence starts.
/// A single letter, or a two-to-four-letter title-case token such as Dr. or Eq., is an abbreviation and stays in the sentence.
pub fn paragraph_states_marked_claims(
    conclusion: &str,
    marker: &str,
    quote: &str,
    paragraph: &str,
) -> bool {
    if quote.trim().is_empty() || !paragraph.contains(quote) {
        return false;
    }
    let claims = marked_claims(conclusion, marker);
    !claims.is_empty()
        && claims.iter().all(|claim| {
            split_sentences(paragraph)
                .iter()
                .any(|sentence| claim_equals_source(claim, marker, &sentence.text))
        })
}
fn marked_claims(conclusion: &str, marker: &str) -> Vec<String> {
    sentences_with_trailing_markers(conclusion)
        .into_iter()
        .filter(|sentence| {
            citation_markers(sentence)
                .iter()
                .any(|found| found == marker)
        })
        .map(|sentence| strip_article_syntax(&sentence))
        .filter(|claim| !claim.is_empty())
        .collect()
}
fn claim_equals_source(article: &str, marker: &str, source: &str) -> bool {
    let source = normalize_claim(source);
    let mut article = normalize_claim(article);
    let extra = count_marker(&article, marker).saturating_sub(count_marker(&source, marker));
    for _ in 0..extra {
        let Some(next) = remove_rightmost_marker(&article, marker) else {
            break;
        };
        article = normalize_claim(&next);
    }
    article = normalize_claim(&strip_trailing_markers_not_in_source(&article, &source));
    article == source
}
fn count_marker(text: &str, marker: &str) -> usize {
    citation_markers(text)
        .iter()
        .filter(|found| found.as_str() == marker)
        .count()
}
fn sentences_with_trailing_markers(text: &str) -> Vec<String> {
    let mut prepared: Vec<String> = Vec::new();
    for sentence in split_sentences(&isolate_marked_table_cells(text)) {
        let (leading, rest) = peel_leading_markers(&sentence.text);
        let same_block = !sentence.new_block;
        if same_block
            && !leading.is_empty()
            && let Some(previous) = prepared.last_mut()
        {
            previous.push(' ');
            previous.push_str(&leading);
        }
        if !rest.is_empty() {
            if !same_block || leading.is_empty() || prepared.is_empty() {
                prepared.push(if leading.is_empty() || !same_block {
                    sentence.text.trim().to_owned()
                } else {
                    format!("{leading} {rest}")
                });
            } else {
                prepared.push(rest);
            }
        } else if !leading.is_empty() && (!same_block || prepared.is_empty()) {
            prepared.push(leading);
        }
    }
    prepared
}
fn peel_leading_markers(sentence: &str) -> (String, String) {
    let mut rest = sentence.trim().to_owned();
    let mut leading = String::new();
    loop {
        let markers = citation_markers(&rest);
        let Some(first) = markers.first() else {
            break;
        };
        if !rest.starts_with(first) {
            break;
        }
        if !leading.is_empty() {
            leading.push(' ');
        }
        leading.push_str(first);
        rest = rest[first.len()..].trim_start().to_owned();
    }
    (leading, rest)
}
fn citation_markers(text: &str) -> Vec<String> {
    let mut markers = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'['
            && let Some(relative) = text[index + 1..].find(']')
        {
            let inside = &text[index + 1..index + 1 + relative];
            if !inside.is_empty() && inside.bytes().all(|byte| byte.is_ascii_digit()) {
                let end = index + 1 + relative + 1;
                markers.push(text[index..end].to_owned());
                index = end;
                continue;
            }
        }
        index += text[index..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }
    markers
}
fn strip_article_syntax(sentence: &str) -> String {
    let without_pipes = sentence.replace('|', "");
    let plain = strip_emphasis_markers(&strip_markdown_links(&without_pipes)).replace('`', "");
    strip_list_prefix(plain.trim())
}
fn remove_rightmost_marker(text: &str, marker: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut found = None;
    while index < bytes.len() {
        if bytes[index] == b'['
            && let Some(relative) = text[index + 1..].find(']')
        {
            let inside = &text[index + 1..index + 1 + relative];
            if !inside.is_empty() && inside.bytes().all(|byte| byte.is_ascii_digit()) {
                let end = index + 1 + relative + 1;
                if text[index..end] == *marker {
                    found = Some((index, end));
                }
                index = end;
                continue;
            }
        }
        index += text[index..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }
    let (mut start, mut end) = found?;
    let before_space = start > 0 && text.as_bytes()[start - 1].is_ascii_whitespace();
    let after = text[end..].trim_start();
    if before_space && (after.is_empty() || after.starts_with(['.', '。', '！', '？', '!', '?']))
    {
        start -= 1;
    } else if before_space && text[end..].starts_with(|ch: char| ch.is_whitespace()) {
        end += text[end..].chars().next().map(char::len_utf8).unwrap_or(1);
    }
    Some(format!("{}{}", &text[..start], &text[end..]))
}
fn strip_trailing_markers_not_in_source(article: &str, source: &str) -> String {
    let mut text = article.trim().to_owned();
    let mut source_body = source.trim().to_owned();
    let terminator = text
        .chars()
        .last()
        .filter(|ch| matches!(ch, '.' | '。' | '！' | '？' | '!' | '?'));
    if terminator.is_some() {
        text.pop();
        text = text.trim_end().to_owned();
    }
    if source_body
        .chars()
        .last()
        .is_some_and(|ch| matches!(ch, '.' | '。' | '！' | '？' | '!' | '?'))
    {
        source_body.pop();
        source_body = source_body.trim_end().to_owned();
    }
    loop {
        let markers = citation_markers(&text);
        let Some(last) = markers.last() else {
            break;
        };
        if !text.ends_with(last.as_str()) || source_body.ends_with(last.as_str()) {
            break;
        }
        text.truncate(text.len() - last.len());
        text = text.trim_end().to_owned();
    }
    if let Some(ch) = terminator {
        text.push(ch);
    }
    text
}
fn strip_emphasis_markers(text: &str) -> String {
    let text = text.replace("**", "").replace("__", "");
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    for (index, ch) in chars.iter().enumerate() {
        if matches!(ch, '*' | '_') {
            let prev_word = index > 0 && chars[index - 1].is_alphanumeric();
            let next_word = chars
                .get(index + 1)
                .is_some_and(|next| next.is_alphanumeric());
            if prev_word && next_word {
                out.push(*ch);
            }
            continue;
        }
        out.push(*ch);
    }
    out
}
fn strip_markdown_links(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(label_end) = after.find(']') {
            let label = &after[..label_end];
            let tail = &after[label_end + 1..];
            if !label.is_empty()
                && !label.bytes().all(|byte| byte.is_ascii_digit())
                && let Some(url_end) = tail.strip_prefix('(').and_then(|url| url.find(')'))
            {
                out.push_str(label);
                rest = &tail[1 + url_end + 1..];
                continue;
            }
        }
        out.push('[');
        rest = &rest[start + 1..];
    }
    out.push_str(rest);
    out
}
fn strip_list_prefix(text: &str) -> String {
    if let Some(rest) = text
        .strip_prefix("- ")
        .or_else(|| text.strip_prefix("* "))
        .or_else(|| text.strip_prefix("+ "))
    {
        return rest.trim().to_owned();
    }
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index > 0 && text[index..].starts_with(". ") {
        return text[index + 2..].trim().to_owned();
    }
    text.to_owned()
}
fn join_markdown_soft_wraps(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        let line = line.trim_end_matches('\r');
        if index > 0 {
            let previous = lines[index - 1].trim_end_matches('\r');
            if markdown_block_break(previous, line) {
                out.push('\n');
            } else if !out.ends_with([' ', '\t']) {
                out.push(' ');
            }
        }
        out.push_str(line);
    }
    out
}
fn markdown_block_break(previous: &str, next: &str) -> bool {
    let previous_text = previous.trim();
    let next_text = next.trim();
    if previous_text.is_empty() || next_text.is_empty() {
        return true;
    }
    if markdown_block_line(next_text) {
        return true;
    }
    markdown_block_line(previous_text) && !list_continuation(previous, next)
}
fn list_continuation(previous: &str, next: &str) -> bool {
    markdown_list_item(previous.trim_start())
        && (next.starts_with("  ") || next.starts_with('\t'))
        && !markdown_block_line(next.trim_start())
}
fn markdown_block_line(trimmed: &str) -> bool {
    trimmed.starts_with('#')
        || trimmed.starts_with('>')
        || trimmed.starts_with("```")
        || markdown_list_item(trimmed)
}
fn markdown_list_item(trimmed: &str) -> bool {
    trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || ordered_list_item(trimmed)
}
fn ordered_list_item(trimmed: &str) -> bool {
    let digits = trimmed
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    digits > 0 && trimmed[digits..].starts_with(". ")
}
struct SplitSentence {
    text: String,
    /// A newline, including a blank line, separates this sentence from the previous one.
    new_block: bool,
}
/// A line with two or more pipes keeps only cells that carry a citation marker.
/// Those cells are separate blocks, so an unmarked cell is not part of the claim.
fn isolate_marked_table_cells(text: &str) -> String {
    text.split('\n')
        .map(marked_table_line)
        .collect::<Vec<_>>()
        .join("\n")
}
fn marked_table_line(line: &str) -> String {
    let line = line.trim_end_matches('\r');
    let trimmed = line.trim();
    if trimmed.matches('|').count() < 2 {
        return line.to_owned();
    }
    let marked: Vec<&str> = trimmed
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .filter(|cell| !cell.is_empty() && !citation_markers(cell).is_empty())
        .collect();
    if marked.is_empty() {
        line.to_owned()
    } else {
        format!("\n\n{}\n\n", marked.join("\n\n"))
    }
}
/// A blank line starts a new block. A citation marker there is not part of the previous sentence.
fn split_sentences(text: &str) -> Vec<SplitSentence> {
    let text = join_markdown_soft_wraps(text);
    let chars: Vec<char> = text.chars().collect();
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut new_block = false;
    for (index, ch) in chars.iter().enumerate() {
        current.push(*ch);
        let newline = *ch == '\n';
        let boundary = newline
            || matches!(ch, '。' | '！' | '？' | '!' | '?')
            || (*ch == '.' && english_sentence_period(&chars, index));
        if boundary {
            let sentence = current.trim().to_owned();
            if sentence.is_empty() {
                new_block = true;
            } else {
                sentences.push(SplitSentence {
                    text: sentence,
                    new_block,
                });
                new_block = newline;
            }
            current.clear();
        }
    }
    let tail = current.trim().to_owned();
    if !tail.is_empty() {
        sentences.push(SplitSentence {
            text: tail,
            new_block,
        });
    }
    sentences
}
fn english_sentence_period(chars: &[char], index: usize) -> bool {
    if chars.get(index + 1).copied() == Some('[') {
        return true;
    }
    let mut look = index + 1;
    while chars.get(look).is_some_and(|ch| ch.is_whitespace()) {
        look += 1;
    }
    let Some(start) = chars.get(look).copied() else {
        return true;
    };
    if start == '[' {
        return true;
    }
    sentence_start(start) && !abbreviation_before(chars, index)
}
fn sentence_start(ch: char) -> bool {
    ch.is_uppercase()
        || ('\u{3400}'..='\u{9fff}').contains(&ch)
        || matches!(ch, '"' | '\'' | '“' | '‘' | '(')
}
/// A single letter, or a short title-case token, is an abbreviation rather than a sentence end.
fn abbreviation_before(chars: &[char], period: usize) -> bool {
    let mut start = period;
    while start > 0 && chars[start - 1].is_ascii_alphanumeric() {
        start -= 1;
    }
    let word = &chars[start..period];
    if word.len() == 1 && word[0].is_ascii_alphabetic() {
        return true;
    }
    (2..=4).contains(&word.len())
        && word[0].is_ascii_uppercase()
        && word[1..].iter().all(|ch| ch.is_ascii_lowercase())
}
fn normalize_claim(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn cited_paragraph(citation: &Citation, accepted: &[ArtifactData]) -> Result<String> {
    let artifact = accepted
        .iter()
        .find(|item| item.reference.id == citation.artifact_id)
        .ok_or_else(|| fault("引用资料包未被授权"))?;
    let source = source_view(artifact, &citation.source_id)?;
    source
        .paragraphs
        .get(citation.paragraph)
        .cloned()
        .ok_or_else(|| fault("引用段落不存在"))
}
pub fn check_report(report: &Report, analysis: &Value, accepted: &[ArtifactData]) -> Result<()> {
    check_analysis(analysis, accepted)?;
    if analysis["decision"] != "ready" || !analysis["gaps"].as_array().is_some_and(|g| g.is_empty())
    {
        return Err(fault("分析仍有决定性缺口，不能接纳为完整回答"));
    }
    if report.answer_status != "answered" || report.citations.is_empty() {
        return Err(fault("资料不足的文章不能通过最终验收"));
    }
    let evidence = analysis["evidence"]
        .as_array()
        .ok_or_else(|| fault("缺少分析证据"))?;
    let mut used = BTreeSet::new();
    let mut quotes = BTreeMap::<String, usize>::new();
    for (index, c) in report.citations.iter().enumerate() {
        let url = verify_citation(c, accepted)?;
        let key = serde_json::to_string(c)?;
        if !used.insert(key)
            || !evidence.iter().any(|v| {
                let mut v = v.clone();
                if let Some(o) = v.as_object_mut() {
                    o.remove("id");
                }
                serde_json::from_value::<Citation>(v).is_ok_and(|e| e == *c)
            })
        {
            return Err(fault("报告引入重复或未分析的引用"));
        }
        let marker = format!("[{}]", index + 1);
        if !report.conclusion.contains(&marker) {
            return Err(fault("引用未在正文中使用"));
        }
        let cjk = c
            .quote
            .chars()
            .filter(|x| ('\u{3400}'..='\u{9fff}').contains(x))
            .count();
        let words = if cjk == 0 {
            c.quote.split_whitespace().count()
        } else {
            cjk + c
                .quote
                .split_whitespace()
                .filter(|w| w.chars().any(|x| x.is_ascii_alphanumeric()))
                .count()
        };
        *quotes.entry(url).or_default() += words;
    }
    if quotes.values().any(|n| *n > 25) {
        return Err(fault("同一来源累计短引文超过二十五词"));
    }
    for part in report.conclusion.split('[').skip(1) {
        if let Some((n, _)) = part.split_once(']')
            && let Ok(n) = n.parse::<usize>()
            && !(1..=report.citations.len()).contains(&n)
        {
            return Err(fault("正文引用编号悬空"));
        }
    }
    Ok(())
}
/// Marked sentences must be stated by their cited paragraphs.
/// `article-integrity@1` does not call this. The claim-paragraph rule does.
pub fn check_claim_sentences(report: &Report, accepted: &[ArtifactData]) -> Result<()> {
    for (index, citation) in report.citations.iter().enumerate() {
        let marker = format!("[{}]", index + 1);
        let paragraph = cited_paragraph(citation, accepted)?;
        if !paragraph_states_marked_claims(&report.conclusion, &marker, &citation.quote, &paragraph)
        {
            return Err(fault("所引段落没有写出带该标记的主张句"));
        }
    }
    Ok(())
}

/// Selection is derived only from actual search responses or explicit user input.
fn candidates(
    seeds: &[String],
    searches: &[Value],
    excluded: &BTreeSet<String>,
) -> Vec<(String, String)> {
    let mut seen = excluded.clone();
    let mut result = Vec::new();
    let mut add = |url: &str, origin: String| {
        if result.len() < 20
            && let Ok(url) = crate::net::normalize_url(url)
            && seen.insert(url.to_string())
        {
            result.push((url.to_string(), origin));
        }
    };
    for url in seeds {
        add(url, "user-input".into());
    }
    for rank in 0..searches
        .iter()
        .filter_map(|s| s["results"].as_array().map(Vec::len))
        .max()
        .unwrap_or(0)
    {
        for search in searches {
            if search["status"] == "discovered"
                && let Some(url) = search["results"][rank]["url"].as_str()
            {
                add(
                    url,
                    search["origin"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            format!("search:{}", search["query"].as_str().unwrap_or(""))
                        }),
                );
            }
        }
    }
    result
}
fn trim_paragraphs(paragraphs: &[Value]) -> (Vec<String>, bool) {
    let mut remaining = 20_000;
    let mut out = Vec::new();
    let mut truncated = false;
    for p in paragraphs {
        let text = p.as_str().expect("trusted normalizer emits strings");
        if remaining == 0 {
            truncated = true;
            break;
        }
        let value: String = text.chars().take(remaining).collect();
        truncated |= value.chars().count() < text.chars().count();
        remaining -= value.chars().count();
        out.push(value);
    }
    (out, truncated)
}
// A previous round remains immutable; only temporary failures are eligible again.
fn exclude_source(fact: &Value) -> bool {
    if fact["usable"] == true || fact["fetch_status"] == "denied" {
        return true;
    }
    match fact["fetch_status"].as_str() {
        Some("captured") => false, // This round may have rejected it at its capacity boundary.
        Some("unavailable") => !matches!(
            fact["http_status"].as_u64(),
            None | Some(408 | 425 | 429 | 500..=599)
        ),
        _ => true,
    }
}
#[derive(Default)]
struct SearchRecovery {
    retries: u8,
    scheduled_wait_ms: u64,
}
impl SearchRecovery {
    fn reserve(&mut self, result: &Value) -> Option<u64> {
        let wait = result["retry_after_ms"].as_u64()?;
        if result["status"] != "unavailable"
            || result["retryable"] != true
            || self.retries >= 2
            || wait > 120_000 - self.scheduled_wait_ms
        {
            return None;
        }
        self.retries += 1;
        self.scheduled_wait_ms += wait;
        Some(wait)
    }
}
async fn collect_searches(
    queries: Vec<String>,
    tools: &Arc<dyn ToolBridge>,
    cancel: &CancellationToken,
) -> Result<Vec<Value>> {
    let mut searches = Vec::new();
    let mut recovery = SearchRecovery::default();
    for query in queries {
        let mut previous = Vec::new();
        let mut waits = Vec::new();
        loop {
            let response = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(fault("资料采集已取消")),
                response = tools.call("inquiry.search", json!({"query":query})) => response?,
            };
            let mut result = response["result"].clone();
            if let Some(wait) = recovery.reserve(&result) {
                previous.push(result);
                waits.push(wait);
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(fault("资料采集已取消")),
                    _ = tokio::time::sleep(std::time::Duration::from_millis(wait)) => {},
                }
                continue;
            }
            if !previous.is_empty() {
                result["recovery"] = json!({"previous_results":previous,"scheduled_wait_ms":waits});
            }
            searches.push(result);
            break;
        }
    }
    Ok(searches)
}
/// Each actual search/fetch is one bounded callback; native assembly is inside one Run.
async fn collect(
    inv: &Invocation,
    tools: &Arc<dyn ToolBridge>,
    round: u8,
    cancel: &CancellationToken,
    native_search: bool,
) -> Result<Value> {
    let mut discoveries = None;
    let mut queries = Vec::new();
    let mut excluded = BTreeSet::new();
    let mut retries = Vec::new();
    let mut prior_sources = BTreeMap::new();
    for reference in &inv.artifacts {
        if !matches!(
            reference.slot.as_str(),
            "plan" | "analysis" | "corpus" | "discoveries"
        ) {
            continue;
        }
        let response = tools
            .call("artifact.read", json!({"id":reference.id}))
            .await?;
        let a = ArtifactData {
            reference: reference.clone(),
            bytes: response["text"]
                .as_str()
                .ok_or_else(|| fault("资料输入不是文本"))?
                .as_bytes()
                .to_vec()
                .into(),
        };
        if a.reference.slot == "discoveries" {
            let value = parse_unique(&a.bytes)?;
            if value["provider"] != "codex-native"
                || value["mode"] != "live"
                || discoveries.is_some()
            {
                return Err(fault("原生发现产物无效或重复"));
            }
            discoveries = Some(
                value["searches"]
                    .as_array()
                    .ok_or_else(|| fault("缺少原生查询记录"))?
                    .clone(),
            );
        }
        if a.reference.slot == "plan" && round == 1 || a.reference.slot == "analysis" && round == 2
        {
            let v = parse_unique(&a.bytes)?;
            queries = v["search_queries"]
                .as_array()
                .ok_or_else(|| fault("缺少声明的查询"))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| fault("查询必须是文本"))
                })
                .collect::<Result<_>>()?;
        }
        if a.reference.slot == "corpus" {
            for s in corpus(&a)?.sources {
                if let Some(url) = s.fact["requested_url"].as_str() {
                    if exclude_source(&s.fact) {
                        excluded.insert(url.to_owned());
                    } else {
                        retries.push((url.to_owned(), s.origin));
                        prior_sources.insert(url.to_owned(),json!({"artifact_id":a.reference.id,"sha256":a.reference.sha256,"source_id":s.source_id}));
                    }
                }
            }
        }
    }
    if queries.len() > 8 {
        return Err(fault("每轮最多八条查询"));
    }
    let mut seeds = inv.inputs["seed_urls"]
        .as_array()
        .ok_or_else(|| fault("缺少用户网址字段"))?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for signal in &inv.signal_facts {
        if let Some(urls) = signal.payload["urls"].as_array() {
            seeds.extend(urls.iter().filter_map(Value::as_str).map(str::to_owned));
        }
    }
    let mut output = Corpus {
        version: 1,
        round,
        sources: vec![],
        searches: vec![],
        limitations: vec![],
    };
    output.searches = if native_search {
        discoveries.ok_or_else(|| fault("本轮缺少受信原生发现回执，禁止HTTP搜索回退"))?
    } else {
        collect_searches(queries, tools, cancel).await?
    };
    let mut selected = candidates(&seeds, &output.searches, &excluded);
    let mut seen = excluded;
    seen.extend(selected.iter().map(|(url, _)| url.clone()));
    for (url, origin) in retries {
        if selected.len() < 20 && seen.insert(url.clone()) {
            selected.push((url, origin));
        }
    }
    let mut archive = BTreeMap::<String, Value>::new();
    for (index, (url, origin)) in selected.into_iter().enumerate() {
        let id = format!("r{round}s{}", index + 1);
        let response = tools
            .call("inquiry.article_fetch", json!({"source_id":id,"url":url}))
            .await?;
        let acquired = &response["result"];
        let mut fact = acquired["fact"].clone();
        if let Some(previous) = prior_sources.get(&url) {
            fact["retry_of"] = previous.clone();
        }
        let mut paragraphs: Vec<String> = serde_json::from_value(acquired["paragraphs"].clone())?;
        if !acquired["raw"].is_null() {
            archive.insert(id.clone(), acquired["raw"].clone());
            if serde_json::to_vec(&archive)?.len() > ARCHIVE_BYTES {
                archive.remove(&id);
                paragraphs.clear();
                fact["usable"] = json!(false);
                fact["reason"] = json!("原文档案容量到界，未接纳为可引用来源");
            }
        }
        let mut source = Source {
            source_id: id.clone(),
            origin,
            fact,
            paragraphs,
        };
        output.sources.push(source.clone());
        if serde_json::to_vec(&output)?.len() > CORPUS_BYTES {
            output.sources.pop();
            archive.remove(&id);
            source.paragraphs.clear();
            source.fact["usable"] = json!(false);
            source.fact["reason"] = json!("资料包容量到界，未接纳为可引用来源");
            output.sources.push(source);
        }
    }
    if output.sources.iter().all(|s| s.fact["usable"] != true) {
        output.limitations.push(
            if output.searches.iter().any(|s| s["status"] == "unavailable") {
                "搜索服务暂不可用；这不是要求用户必须提供网址".into()
            } else {
                "本轮未取得新的可用保存资料".into()
            },
        );
    }
    if serde_json::to_vec(&output)?.len() > CORPUS_BYTES {
        return Err(fault("资料包处置记录超过容量边界"));
    }
    Ok(json!({"corpus":output,"archive":archive}))
}
fn archive_body(bytes: &[u8]) -> Result<Value> {
    Ok(serde_json::to_value(ArchivedBody::capture(bytes)?)?)
}
pub struct FetchSource;
#[async_trait]
impl DomainTool for FetchSource {
    fn spec(&self) -> ToolSpec {
        ToolSpec { id:"inquiry.article_fetch".into(), description:"Native collection only: fetch and preserve one public source within the callback deadline.".into(), input_schema:json!({"type":"object","properties":{"source_id":{"type":"string","pattern":"^r[12]s[0-9]+$"},"url":{"type":"string","maxLength":4096}},"required":["source_id","url"],"additionalProperties":false}) }
    }
    async fn call(&self, c: ToolContext, args: Value) -> Result<ToolOutput> {
        if !matches!(c.goal().map(|g| &g.role), Some(GoalRunRole::NodeMember{node_id,..}) if node_id=="collect" || node_id=="supplement")
        {
            return Err(Error::new(
                "capability_denied",
                "/collect",
                "资料获取只能由锁定的原生节点执行",
            ));
        }
        let id = args["source_id"]
            .as_str()
            .ok_or_else(|| fault("缺少来源身份"))?;
        let url = args["url"].as_str().ok_or_else(|| fault("缺少来源网址"))?;
        let acquired = crate::domain::acquire_source(&c, id, url).await?;
        let mut fact = acquired.payload;
        let snapshot = acquired
            .files
            .iter()
            .find(|f| f.slot.starts_with("capture.snapshot."));
        let (paragraphs, truncated) = match snapshot {
            Some(f) => trim_paragraphs(
                f.value()?["paragraphs"]
                    .as_array()
                    .ok_or_else(|| fault("规范化段落缺失"))?,
            ),
            None => (vec![], false),
        };
        fact["truncated"] = json!(truncated || fact["truncated"] == true);
        fact["transform_version"] = json!("html-text-v2-20k");
        fact["normalized_sha256"] = json!(crate::domain::hash(&serde_json::to_vec(&paragraphs)?));
        let raw = acquired
            .files
            .iter()
            .find(|f| f.slot.starts_with("capture.raw."))
            .map(|f| archive_body(&f.bytes))
            .transpose()?;
        Ok(ToolOutput::json(
            json!({"fact":fact,"paragraphs":paragraphs,"raw":raw}),
        ))
    }
}

const SOURCE_PAGE_MAX: usize = 8;
const SOURCE_PAGE_DEFAULT: usize = 5;
const SOURCE_PARAGRAPH_CHARS: usize = 4000;
const INDEX_HITS: usize = 20;
const INDEX_EXCERPT_CHARS: usize = 180;
fn source_read_limits() -> Value {
    json!({"max_paragraphs":SOURCE_PAGE_MAX,"default_paragraphs":SOURCE_PAGE_DEFAULT,"max_chars_per_paragraph":SOURCE_PARAGRAPH_CHARS})
}
struct IndexHits {
    hits: Vec<Value>,
    omitted: usize,
}

fn keyword_index_hits(sources: &[Source], keyword: &str) -> IndexHits {
    let key = keyword.to_lowercase();
    let mut hits = Vec::new();
    let mut omitted = 0;
    for source in sources {
        if source.fact["usable"] != true {
            continue;
        }
        let Some((idx, paragraph)) = source
            .paragraphs
            .iter()
            .enumerate()
            .find(|(_, paragraph)| paragraph.to_lowercase().contains(&key))
        else {
            continue;
        };
        if hits.len() == INDEX_HITS {
            omitted += 1;
            continue;
        }
        let excerpt: String = paragraph.chars().take(INDEX_EXCERPT_CHARS).collect();
        hits.push(json!({
            "source_id": source.source_id,
            "paragraph": idx,
            "excerpt": excerpt,
            "title": source.fact["title"].clone(),
            "final_url": source.fact["final_url"].clone(),
        }));
    }
    IndexHits { hits, omitted }
}

fn source_paragraph_page(
    source: &Source,
    keyword: &str,
    start: usize,
    limit: usize,
) -> (Vec<Value>, Option<usize>) {
    let mut matches = source
        .paragraphs
        .iter()
        .enumerate()
        .skip(start)
        .filter(|(_, p)| keyword.is_empty() || p.to_lowercase().contains(keyword))
        .peekable();
    let paragraphs=matches.by_ref().take(limit).map(|(i,p)|json!({"paragraph":i,"text":p.chars().take(SOURCE_PARAGRAPH_CHARS).collect::<String>(),"excerpted":p.chars().count()>SOURCE_PARAGRAPH_CHARS})).collect();
    let next_start = matches.peek().map(|(i, _)| *i);
    (paragraphs, next_start)
}
pub struct ReadSource;
#[async_trait]
impl DomainTool for ReadSource {
    fn spec(&self) -> ToolSpec {
        ToolSpec { id: "inquiry.article_source".into(), description: "Read exact saved paragraphs. Omit source_id for the source index and a valid read_example. A keyword with no source_id returns one short exact excerpt per matching saved source and does not open that source. Per call limit is 1..8 (default5), NOT total_paragraphs; larger limits are rejected. Filter by keyword and follow next_start for another page. Use exact package id/hash, source_id and zero-based paragraph for citations.".into(),
            input_schema: json!({"type":"object","properties":{"artifact_id":{"type":"string"},"source_id":{"type":"string"},"keyword":{"type":"string","maxLength":200},"start":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":SOURCE_PAGE_MAX,"description":"Paragraph count for this call: 1..8, default5. Never pass total_paragraphs; use next_start for another page."}},"required":["artifact_id"],"additionalProperties":false}) }
    }
    async fn call(&self, c: ToolContext, args: Value) -> Result<ToolOutput> {
        let id = args["artifact_id"]
            .as_str()
            .ok_or_else(|| fault("缺少资料包身份"))?;
        let artifact = c
            .accepted
            .iter()
            .find(|a| a.reference.id == id && a.reference.slot == "corpus")
            .ok_or_else(|| fault("资料包未被当前阶段授权"))?;
        let Some(source_id) = args["source_id"].as_str().filter(|value| !value.is_empty()) else {
            let package = corpus(artifact)?;
            let example=package.sources.iter().find(|s|s.fact["usable"]==true).map(|s|json!({"artifact_id":id,"source_id":s.source_id,"start":0,"limit":SOURCE_PAGE_DEFAULT}));
            let keyword = args["keyword"].as_str().unwrap_or("").trim();
            if !keyword.is_empty() {
                let found = keyword_index_hits(&package.sources, keyword);
                return Ok(ToolOutput::json(json!({
                    "artifact_id": id,
                    "sha256": artifact.reference.sha256,
                    "keyword": keyword,
                    "hits": found.hits,
                    "omitted": found.omitted,
                    "read_limits": source_read_limits(),
                    "read_example": example,
                })));
            }
            return Ok(ToolOutput::json(
                json!({"artifact_id":id,"sha256":artifact.reference.sha256,"sources":package.sources.iter().map(|s| json!({"source_id":s.source_id,"origin":s.origin,"fact":s.fact,"paragraphs":s.paragraphs.len()})).collect::<Vec<_>>(),"limitations":package.limitations,"read_limits":source_read_limits(),"read_example":example}),
            ));
        };
        let source = source_view(artifact, source_id)?;
        let keyword = args["keyword"].as_str().unwrap_or("").to_lowercase();
        let start = args["start"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(SOURCE_PAGE_DEFAULT as u64) as usize;
        let (paragraphs, next_start) = source_paragraph_page(&source, &keyword, start, limit);
        Ok(ToolOutput::json(
            json!({"artifact_id":id,"sha256":artifact.reference.sha256,"source_id":source.source_id,"fact":source.fact,"total_paragraphs":source.paragraphs.len(),"paragraphs":paragraphs,"read_limits":source_read_limits(),"next_start":next_start,"has_more":next_start.is_some()}),
        ))
    }
}
pub enum Native {
    Collect,
    CollectNative,
    Integrity,
}
#[async_trait]
impl Executor for Native {
    fn capabilities(&self) -> ExecutorCapabilities {
        ExecutorCapabilities::MANAGED
    }
    fn enforces_json_candidate_limit(&self) -> bool {
        true
    }
    async fn execute(
        &self,
        inv: Invocation,
        tools: Arc<dyn ToolBridge>,
        cancel: CancellationToken,
    ) -> ExecutionOutcome {
        let result: Result<()> = async {
            match self {
                Self::Collect | Self::CollectNative => {
                    let GoalRunRole::NodeMember { node_id, .. } = tools.goal_context().ok_or_else(|| fault("缺少 Goal 归属"))?.role else { return Err(fault("原生资料节点归属无效")); };
                    let response = collect(&inv, &tools, if node_id=="collect" {1} else {2}, &cancel, matches!(self,Self::CollectNative)).await?;
                    for slot in ["corpus", "archive"] {
                        tools.call("artifact.write", json!({"slot":slot,"value":response[slot]})).await?;
                    }
                }
                Self::Integrity => {
                    let input: CheckInput = serde_json::from_value(inv.stage_input)?;
                    let CheckSubject::Artifacts { targets, .. } = &input.subject else { return Err(fault("需要确切报告目标")); };
                    let report: Report = serde_json::from_value(targets["article"].value.clone())?;
                    let accepted = targets.values().map(|t| ArtifactData { reference: ArtifactRef { id:t.artifact_id.clone(),sha256:t.sha256.clone(),slot:if t.value.get("sources").is_some(){"corpus"}else{"input"}.into(),state_id:"goal".into(),visit:1 },bytes:serde_json::to_vec(&t.value).expect("JSON value").into() }).collect::<Vec<_>>();
                    let structural = check_report(&report, &targets["analysis"].value, &accepted).map_err(|error| error.message);
                    let claim = input.rules.rules.iter().any(|rule| rule.id == "claim_paragraph").then(|| check_claim_sentences(&report, &accepted).map_err(|error| error.message));
                    let mut results = Vec::new();
                    for rule in &input.rules.rules {
                        let (verdict, message) = match rule.id.as_str() {
                            "citation_integrity" => match &structural {
                                Ok(()) => ("pass", "引用版本、来源段落、分析血缘和短引文边界通过".to_owned()),
                                Err(message) => ("fail", message.clone()),
                            },
                            "claim_paragraph" => match &claim {
                                Some(Ok(())) => ("pass", "所引段落写出了带该标记的主张句".to_owned()),
                                Some(Err(message)) => ("fail", message.clone()),
                                None => ("error", "主张句规则未执行".to_owned()),
                            },
                            _ => ("error", format!("未知验收规则 {}", rule.id)),
                        };
                        results.push(json!({"id":rule.id,"verdict":verdict,"message":message,"evidence_refs":input.evidence_refs}));
                    }
                    let verdict = if results.iter().any(|result| result["verdict"] == "error") { "error" } else if results.iter().any(|result| result["verdict"] == "fail") { "fail" } else { "pass" };
                    tools.call("artifact.write", json!({"slot":"evaluation","value":{"protocol_version":1,"verdict":verdict,"results":results,"evidence_refs":input.evidence_refs,"violations":[]}})).await?;
                }
            }
            Ok(())
        }.await;
        match result {
            Ok(()) => ExecutionOutcome::Completed {
                usage: json!({"native_model":false}),
            },
            Err(_) if cancel.is_cancelled() => ExecutionOutcome::Cancelled,
            Err(e) => ExecutionOutcome::Failed {
                code: e.code,
                message: e.message,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_search_selection_is_round_robin_scoped_and_deduplicated() {
        let searches = vec![
            json!({"query":"a","status":"discovered","results":[{"url":"https://a.example/1"},{"url":"https://a.example/2"}]}),
            json!({"query":"b","status":"discovered","results":[{"url":"https://b.example/1"}]}),
            json!({"query":"bad","status":"unavailable","results":[{"url":"https://invented.example/"}]}),
        ];
        let found = candidates(
            &["https://seed.example/".into()],
            &searches,
            &BTreeSet::from(["https://a.example/2".into()]),
        );
        assert_eq!(
            found,
            vec![
                ("https://seed.example/".into(), "user-input".into()),
                ("https://a.example/1".into(), "search:a".into()),
                ("https://b.example/1".into(), "search:b".into())
            ]
        );
    }
    #[test]
    fn keyword_index_points_at_one_saved_paragraph_and_skips_the_rest() {
        let matched = "默认分词器按空格切，CJK 连续串成一个 token。".repeat(8);
        let sources = vec![
            Source {
                source_id: "r1s4".into(),
                origin: "search".into(),
                fact: json!({"usable": true, "title": "bench", "final_url": "https://example.com/bench"}),
                paragraphs: vec!["intro".into(), matched.clone()],
            },
            Source {
                source_id: "r1s5".into(),
                origin: "search".into(),
                fact: json!({"usable": true, "title": "other", "final_url": "https://example.com/other"}),
                paragraphs: vec!["no term here".into()],
            },
            Source {
                source_id: "r1s19".into(),
                origin: "search".into(),
                fact: json!({"usable": false, "title": "", "final_url": "https://example.com/denied"}),
                paragraphs: vec!["CJK 连续串".into()],
            },
        ];
        let found = keyword_index_hits(&sources, "CJK");
        assert_eq!(found.omitted, 0);
        assert_eq!(found.hits.len(), 1);
        assert_eq!(found.hits[0]["source_id"], "r1s4");
        assert_eq!(found.hits[0]["paragraph"], 1);
        let excerpt = found.hits[0]["excerpt"].as_str().unwrap();
        assert!(excerpt.chars().count() <= INDEX_EXCERPT_CHARS);
        assert!(matched.starts_with(excerpt));
        assert!(keyword_index_hits(&sources, "不存在的词").hits.is_empty());
    }

    #[test]
    fn saved_paragraph_limit_is_character_based_and_explicit() {
        let (saved, truncated) = trim_paragraphs(&[json!("中".repeat(21000)), json!("later")]);
        assert!(truncated);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].chars().count(), 20000);
    }
}

#[cfg(test)]
#[path = "article/tests.rs"]
mod recovery_tests;
