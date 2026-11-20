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
/// List marks, links, emphasis, and table pipes are article syntax. A citation marker inside a code span or a link URL is not a citation.
/// An underscore inside a word is not emphasis. Emphasis and links inside a code span stay literal.
/// A table cell is compared alone only when its row belongs to a table that has a separator line. A row may contain one pipe.
/// A citation marker before a closing quote or parenthesis stays on that sentence, and the space before the marker is removed.
/// The citation marker may sit inside the sentence. A different in-sentence marker, such as [12], stays.
/// The remaining sentence must equal one whole sentence of the paragraph. A fragment inside a longer sentence does not pass.
/// A Markdown soft wrap stays inside that sentence. A blank line, heading, or new list item starts another.
/// A period ends an English sentence at the end of the text, or when whitespace and a new sentence follow. The new sentence may start with a letter or a digit.
/// Emphasis closers sitting immediately after that punctuation stay on the sentence. A sentence end in a link label ends the sentence after the whole link. Punctuation inside the URL does not.
/// A decimal or version point such as 2.0 is not a sentence end. A listed abbreviation such as Dr., Eq., or Inc. stays in the sentence, even when a citation marker follows it.
/// The final period of an initialism such as U.S. ends the sentence only when a new sentence follows a clause, as in "We operate in the U.S. Sales rose." A capitalized name stays on the initialism, so "U.S. Government" and "U.K. Government" remain different sentences. U.S. policy stays one sentence. A single letter after a word can end a sentence, as in "Plan A. Results improved." A. Smith stays one sentence. A hashtag without a space is not a heading, so a soft wrap keeps it in the sentence.
/// A closing quote or parenthesis after 。！？ stays on that sentence. A Markdown heading is article syntax only when the hash marks are followed by a space.
pub fn paragraph_states_marked_claims(
    conclusion: &str,
    marker: &str,
    quote: &str,
    paragraph: &str,
) -> bool {
    let sentences: Vec<String> = split_sentences(paragraph)
        .into_iter()
        .map(|sentence| sentence.text)
        .collect();
    paragraph_matches(
        &marked_claims(conclusion, marker),
        marker,
        quote,
        paragraph,
        &sentences,
    )
}
fn paragraph_matches(
    claims: &[String],
    marker: &str,
    quote: &str,
    paragraph: &str,
    sentences: &[String],
) -> bool {
    if quote.trim().is_empty() || !paragraph.contains(quote) || claims.is_empty() {
        return false;
    }
    claims.iter().all(|claim| {
        sentences
            .iter()
            .any(|sentence| claim_equals_source(claim, marker, sentence))
    })
}
fn marked_claims(conclusion: &str, marker: &str) -> Vec<String> {
    claims_by_marker(conclusion)
        .remove(marker)
        .unwrap_or_default()
}
fn claims_by_marker(conclusion: &str) -> BTreeMap<String, Vec<String>> {
    let mut claims: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for sentence in sentences_with_trailing_markers(conclusion) {
        if strip_article_syntax(&sentence).trim().is_empty() {
            continue;
        }
        for marker in citation_markers(&sentence) {
            claims.entry(marker).or_default().push(sentence.clone());
        }
    }
    claims
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
    // Drop article syntax only after citation markers are removed, so a `[n]`
    // inside a code span stays literal text instead of becoming another citation.
    article = normalize_claim(&strip_article_syntax(&article));
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
    citation_marker_ranges(text)
        .into_iter()
        .map(|(start, end)| text[start..end].to_owned())
        .collect()
}
/// Byte ranges of `[digits]` citations. A marker inside a code span, or inside a Markdown link URL, is not a citation.
fn citation_marker_ranges(text: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let mut char_to_byte = Vec::with_capacity(chars.len() + 1);
    let mut byte = 0;
    for ch in &chars {
        char_to_byte.push(byte);
        byte += ch.len_utf8();
    }
    char_to_byte.push(byte);
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if let Some(end) = code_span_end(&chars, index) {
            index = end + 1;
            continue;
        }
        if let Some(link_end) = markdown_link_end(&chars, index) {
            let label_close = chars[index + 1..=link_end]
                .iter()
                .position(|ch| *ch == ']')
                .map(|offset| index + 1 + offset);
            if let Some(label_close) = label_close {
                let label: String = chars[index + 1..label_close].iter().collect();
                let label_byte = char_to_byte[index + 1];
                for (start, end) in citation_marker_ranges(&label) {
                    ranges.push((label_byte + start, label_byte + end));
                }
            }
            index = link_end + 1;
            continue;
        }
        if let Some(end) = digit_marker_end(&chars, index) {
            ranges.push((char_to_byte[index], char_to_byte[end + 1]));
            index = end + 1;
            continue;
        }
        index += 1;
    }
    ranges
}
fn digit_marker_end(chars: &[char], index: usize) -> Option<usize> {
    if chars.get(index) != Some(&'[') {
        return None;
    }
    let mut cursor = index + 1;
    if !chars.get(cursor).is_some_and(|ch| ch.is_ascii_digit()) {
        return None;
    }
    while chars.get(cursor).is_some_and(|ch| ch.is_ascii_digit()) {
        cursor += 1;
    }
    if chars.get(cursor) == Some(&']') {
        Some(cursor)
    } else {
        None
    }
}
fn strip_article_syntax(sentence: &str) -> String {
    let protected = map_outside_code_spans(sentence, |text| {
        strip_emphasis_markers(&strip_markdown_links(text))
    });
    let plain = strip_code_spans(&protected);
    strip_block_prefix(plain.trim())
}
fn map_outside_code_spans(text: &str, mut transform: impl FnMut(&str) -> String) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    let mut segment = 0;
    while index < chars.len() {
        if let Some(end) = code_span_end(&chars, index) {
            let outside: String = chars[segment..index].iter().collect();
            out.push_str(&transform(&outside));
            out.extend(chars[index..=end].iter().copied());
            index = end + 1;
            segment = index;
            continue;
        }
        index += 1;
    }
    let outside: String = chars[segment..].iter().collect();
    out.push_str(&transform(&outside));
    out
}
fn strip_code_spans(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let marks = after.chars().take_while(|ch| *ch == '`').count();
        let body = &after[marks..];
        let fence = &after[..marks];
        if let Some(end) = body.find(fence) {
            out.push_str(&body[..end]);
            rest = &body[end + fence.len()..];
        } else {
            out.push_str(fence);
            rest = body;
        }
    }
    out.push_str(rest);
    out
}
fn strip_block_prefix(text: &str) -> String {
    let mut rest = text.trim().to_owned();
    loop {
        let trimmed = rest.trim();
        if let Some(after) = trimmed.strip_prefix('>') {
            rest = after.trim_start().to_owned();
        } else if let Some(after) = heading_body(trimmed) {
            rest = after.to_owned();
        } else {
            let listed = strip_list_prefix(trimmed);
            if listed.trim() == trimmed {
                break;
            }
            rest = listed;
        }
    }
    rest
}
fn heading_body(text: &str) -> Option<&str> {
    let marks = text.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&marks) && text.as_bytes().get(marks) == Some(&b' ') {
        Some(text[marks..].trim_start())
    } else {
        None
    }
}
fn remove_rightmost_marker(text: &str, marker: &str) -> Option<String> {
    let (mut start, mut end) = citation_marker_ranges(text)
        .into_iter()
        .rev()
        .find(|(start, end)| &text[*start..*end] == marker)?;
    let before_space = start > 0 && text.as_bytes()[start - 1].is_ascii_whitespace();
    let after_raw = &text[end..];
    let after = after_raw.trim_start();
    let matching_closer = after.chars().next().is_some_and(|ch| {
        sentence_closer(ch) && delimiter_stack(&text[..start]).last() == Some(&ch)
    });
    if before_space
        && (after.is_empty()
            || after.starts_with(['.', '。', '！', '？', '!', '?'])
            || matching_closer)
    {
        start -= 1;
    } else if before_space && after_raw.starts_with(|ch: char| ch.is_whitespace()) {
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
    let text = strip_paired(text, "**", false);
    let text = strip_paired(&text, "__", true);
    let text = strip_paired(&text, "*", false);
    strip_paired(&text, "_", true)
}
fn strip_paired(text: &str, marker: &str, reject_intra_word: bool) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(marker) {
        let after = &rest[start + marker.len()..];
        let opener = !after.starts_with(char::is_whitespace) && !after.is_empty();
        if opener && let Some(end) = after.find(marker) {
            let inside = &after[..end];
            let followed_by_word = after[end + marker.len()..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphanumeric());
            let preceded_by_word = rest[..start]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_alphanumeric());
            let intra_word = reject_intra_word && (preceded_by_word || followed_by_word);
            if !intra_word
                && !inside.is_empty()
                && !inside.ends_with(char::is_whitespace)
                && !inside.contains('\n')
            {
                out.push_str(&rest[..start]);
                out.push_str(inside);
                rest = &after[end + marker.len()..];
                continue;
            }
        }
        out.push_str(&rest[..start + marker.len()]);
        rest = after;
    }
    out.push_str(rest);
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
                && let Some(url_end) = matching_paren(tail)
            {
                out.push_str(label);
                rest = &tail[url_end + 1..];
                continue;
            }
        }
        out.push('[');
        rest = &rest[start + 1..];
    }
    out.push_str(rest);
    out
}
fn matching_paren(text: &str) -> Option<usize> {
    if !text.starts_with('(') {
        return None;
    }
    let mut depth = 0;
    for (index, ch) in text.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
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
        let previous = index
            .checked_sub(1)
            .map(|prior| lines[prior].trim_end_matches('\r'));
        let quote_continued =
            previous.is_some_and(|prior| blockquote_continuation(prior.trim(), line.trim()));
        if let Some(prior) = previous {
            if markdown_block_break(prior, line) {
                out.push('\n');
            } else if !out.ends_with([' ', '\t']) {
                out.push(' ');
            }
        }
        if quote_continued {
            out.push_str(blockquote_body(line));
        } else {
            out.push_str(line);
        }
    }
    out
}
fn markdown_block_break(previous: &str, next: &str) -> bool {
    let previous_text = previous.trim();
    let next_text = next.trim();
    if previous_text.is_empty() || next_text.is_empty() {
        return true;
    }
    if blockquote_continuation(previous_text, next_text) {
        return false;
    }
    if markdown_block_line(next_text) {
        return true;
    }
    markdown_block_line(previous_text) && !list_continuation(previous, next)
}
fn blockquote_continuation(previous: &str, next: &str) -> bool {
    previous.starts_with('>')
        && next.starts_with('>')
        && !blockquote_body(previous).is_empty()
        && !blockquote_body(next).is_empty()
}
fn blockquote_body(line: &str) -> &str {
    let mut rest = line.trim();
    while let Some(after) = rest.strip_prefix('>') {
        rest = after.trim_start();
    }
    rest
}
fn list_continuation(previous: &str, next: &str) -> bool {
    markdown_list_item(previous.trim_start())
        && (next.starts_with("  ") || next.starts_with('\t'))
        && !markdown_block_line(next.trim_start())
}
fn markdown_block_line(trimmed: &str) -> bool {
    heading_body(trimmed).is_some()
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
/// A Markdown table keeps only cells that carry a citation marker.
/// Those cells are separate blocks, so an unmarked cell is not part of the claim.
/// A line of prose that merely contains pipes is not a table unless that block has a separator row.
fn isolate_marked_table_cells(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let in_table = table_line_mask(&lines);
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if in_table[index] {
                marked_table_line(line)
            } else {
                line.trim_end_matches('\r').to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn table_line_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut index = 0;
    while index < lines.len() {
        if !table_row_line(lines[index]) {
            index += 1;
            continue;
        }
        let start = index;
        while index < lines.len() && table_row_line(lines[index]) {
            index += 1;
        }
        if (start..index).any(|cursor| table_separator_line(lines[cursor])) {
            for item in &mut mask[start..index] {
                *item = true;
            }
        }
    }
    mask
}
fn table_row_line(line: &str) -> bool {
    let trimmed = line.trim().trim_end_matches('\r');
    table_separator_line(line) || trimmed.contains('|')
}
fn table_separator_line(line: &str) -> bool {
    let trimmed = line.trim().trim_end_matches('\r');
    if !trimmed.contains('|') {
        return false;
    }
    let cells = table_cells(trimmed);
    !cells.is_empty() && cells.iter().all(|cell| separator_cell(cell))
}
fn separator_cell(cell: &str) -> bool {
    let mut chars = cell.trim().chars().peekable();
    if chars.peek() == Some(&':') {
        chars.next();
    }
    let mut dashes = 0;
    while chars.peek() == Some(&'-') {
        chars.next();
        dashes += 1;
    }
    if chars.peek() == Some(&':') {
        chars.next();
    }
    dashes >= 3 && chars.next().is_none()
}
fn marked_table_line(line: &str) -> String {
    let line = line.trim_end_matches('\r');
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return line.to_owned();
    }
    let marked: Vec<String> = table_cells(trimmed)
        .into_iter()
        .filter(|cell| !citation_markers(cell).is_empty())
        .collect();
    if marked.is_empty() {
        line.to_owned()
    } else {
        format!("\n\n{}\n\n", marked.join("\n\n"))
    }
}
fn table_cells(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\\' && chars.get(index + 1) == Some(&'|') {
            current.push('|');
            index += 2;
            continue;
        }
        if let Some(end) = code_span_end(&chars, index) {
            for ch in &chars[index..=end] {
                current.push(*ch);
            }
            index = end + 1;
            continue;
        }
        if chars[index] == '|' {
            let cell = current.trim().to_owned();
            if !cell.is_empty() {
                cells.push(cell);
            }
            current.clear();
            index += 1;
            continue;
        }
        current.push(chars[index]);
        index += 1;
    }
    let cell = current.trim().to_owned();
    if !cell.is_empty() {
        cells.push(cell);
    }
    cells
}
/// A blank line starts a new block. A citation marker there is not part of the previous sentence.
fn split_sentences(text: &str) -> Vec<SplitSentence> {
    let text = join_markdown_soft_wraps(text);
    let chars: Vec<char> = text.chars().collect();
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut new_block = false;
    let mut index = 0;
    while index < chars.len() {
        if let Some(end) = code_span_end(&chars, index) {
            for ch in &chars[index..=end] {
                current.push(*ch);
            }
            index = end + 1;
            continue;
        }
        if let Some(end) = markdown_link_end(&chars, index) {
            let ends_sentence = link_label_has_sentence_end(&chars, index, end);
            for ch in &chars[index..=end] {
                current.push(*ch);
            }
            index = end + 1;
            if ends_sentence {
                let sentence = current.trim().to_owned();
                if sentence.is_empty() {
                    new_block = true;
                } else {
                    sentences.push(SplitSentence {
                        text: sentence,
                        new_block,
                    });
                    new_block = false;
                }
                current.clear();
            }
            continue;
        }
        let ch = chars[index];
        current.push(ch);
        let newline = ch == '\n';
        let punctuation = matches!(ch, '。' | '！' | '？' | '!' | '?')
            || (ch == '.' && english_sentence_period(&chars, index));
        if punctuation {
            let period = index;
            index += 1;
            let tail = trailing_syntax_end(&chars, index);
            while index < tail {
                current.push(chars[index]);
                index += 1;
            }
            if let Some(end) = marker_before_matching_closer(&chars, period, index) {
                current.extend(chars[index..end].iter().copied());
                index = end;
            }
        } else {
            index += 1;
        }
        if punctuation || newline {
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
fn code_span_end(chars: &[char], start: usize) -> Option<usize> {
    if chars.get(start) != Some(&'`') {
        return None;
    }
    let mut marks = 0;
    while chars.get(start + marks) == Some(&'`') {
        marks += 1;
    }
    let mut index = start + marks;
    while index < chars.len() {
        if chars[index] == '`' {
            let mut close = 0;
            while chars.get(index + close) == Some(&'`') {
                close += 1;
            }
            if close >= marks {
                return Some(index + marks - 1);
            }
            index += close;
            continue;
        }
        index += 1;
    }
    None
}
fn markdown_link_end(chars: &[char], start: usize) -> Option<usize> {
    if chars.get(start) != Some(&'[') {
        return None;
    }
    let mut index = start + 1;
    let label_start = index;
    while index < chars.len() && chars[index] != ']' {
        index += 1;
    }
    if index >= chars.len() {
        return None;
    }
    let label: String = chars[label_start..index].iter().collect();
    if label.is_empty() || label.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if chars.get(index + 1) != Some(&'(') {
        return None;
    }
    let mut depth = 0;
    let mut cursor = index + 1;
    while cursor < chars.len() {
        match chars[cursor] {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}
fn delimiter_stack(text: &str) -> Vec<char> {
    let chars: Vec<char> = text.chars().collect();
    delimiter_stack_before(&chars, chars.len())
}
fn delimiter_stack_before(chars: &[char], end: usize) -> Vec<char> {
    let mut stack = Vec::new();
    let mut index = 0;
    while index < end {
        if let Some(span) = code_span_end(chars, index).or_else(|| markdown_link_end(chars, index))
        {
            index = span + 1;
            continue;
        }
        match chars[index] {
            '"' => {
                if stack.last() == Some(&'"') {
                    stack.pop();
                } else {
                    stack.push('"');
                }
            }
            '“' => stack.push('”'),
            '”' if stack.last() == Some(&'”') => {
                stack.pop();
            }
            '(' => stack.push(')'),
            ')' if stack.last() == Some(&')') => {
                stack.pop();
            }
            '（' => stack.push('）'),
            '）' if stack.last() == Some(&'）') => {
                stack.pop();
            }
            '「' => stack.push('」'),
            '」' if stack.last() == Some(&'」') => {
                stack.pop();
            }
            '『' => stack.push('』'),
            '』' if stack.last() == Some(&'』') => {
                stack.pop();
            }
            _ => {}
        }
        index += 1;
    }
    stack
}
/// A citation between a sentence end and the closer that finishes an open quote or parenthesis stays on that sentence.
fn marker_before_matching_closer(chars: &[char], period: usize, index: usize) -> Option<usize> {
    let mut stack = delimiter_stack_before(chars, period);
    if stack.is_empty() {
        return None;
    }
    let mut look = index;
    while chars.get(look).is_some_and(|ch| ch.is_whitespace()) {
        look += 1;
    }
    let marker_end = digit_marker_end(chars, look)?;
    look = marker_end + 1;
    while chars.get(look).is_some_and(|ch| ch.is_whitespace()) {
        look += 1;
    }
    let before_closers = look;
    while chars.get(look).is_some_and(|ch| stack.last() == Some(ch)) {
        stack.pop();
        look += 1;
    }
    (look > before_closers).then_some(look)
}
fn sentence_closer(ch: char) -> bool {
    matches!(
        ch,
        '"' | '\'' | '”' | '’' | '」' | '』' | ')' | '）' | '］' | '}' | '｝'
    )
}
/// Quote and parenthesis closers, then emphasis closers, when they sit on the punctuation.
/// An emphasis run is kept only when whitespace, the end, or another closer follows it.
fn trailing_syntax_end(chars: &[char], start: usize) -> usize {
    let mut look = start;
    loop {
        while chars.get(look).is_some_and(|ch| sentence_closer(*ch)) {
            look += 1;
        }
        let marks = look;
        while chars
            .get(look)
            .is_some_and(|ch| matches!(ch, '*' | '_' | '`'))
        {
            look += 1;
        }
        if look == marks {
            return look;
        }
        match chars.get(look).copied() {
            None => return look,
            Some(ch) if ch.is_whitespace() => return look,
            Some(ch) if sentence_closer(ch) => continue,
            _ => return marks,
        }
    }
}
/// A period inside a link label is a sentence end in the visible text.
/// The URL stays on that sentence, and the next sentence starts after the link.
fn link_label_has_sentence_end(chars: &[char], start: usize, link_end: usize) -> bool {
    let Some(relative) = chars[start + 1..=link_end].iter().position(|ch| *ch == ']') else {
        return false;
    };
    let label_close = start + 1 + relative;
    let label = &chars[start + 1..label_close];
    let mut visible = label.to_vec();
    visible.extend_from_slice(&chars[link_end + 1..]);
    let mut index = 0;
    while index < label.len() {
        if let Some(end) = code_span_end(label, index) {
            index = end + 1;
            continue;
        }
        let ch = label[index];
        let punctuation = matches!(ch, '。' | '！' | '？' | '!' | '?')
            || (ch == '.' && english_sentence_period(&visible, index));
        if punctuation {
            return true;
        }
        index += 1;
    }
    false
}
fn english_sentence_period(chars: &[char], index: usize) -> bool {
    let mut look = trailing_syntax_end(chars, index + 1);
    let after_closers = look;
    if chars.get(look).copied() == Some('[') {
        return !abbreviation_before(chars, index);
    }
    while chars.get(look).is_some_and(|ch| ch.is_whitespace()) {
        look += 1;
    }
    let Some(start) = chars.get(look).copied() else {
        return true;
    };
    if look == after_closers {
        return false;
    }
    if start == '[' {
        return !abbreviation_before(chars, index);
    }
    while chars
        .get(look)
        .is_some_and(|ch| matches!(ch, '*' | '_' | '`'))
    {
        look += 1;
    }
    let Some(start) = chars.get(look).copied() else {
        return true;
    };
    if start == '[' {
        return !abbreviation_before(chars, index);
    }
    sentence_start(start) && !abbreviation_before(chars, index)
}
fn sentence_start(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '"' | '\'' | '“' | '‘' | '(' | '（')
}
/// A single letter, or one of these written abbreviations, is not a sentence end
/// unless the next word opens a sentence, as in `Acme Inc. Revenue rose.`
/// A company suffix also ends the sentence when a capitalized clause follows, as in `Acme Inc. Customers responded.`
/// The last letter of an initialism such as `U.S.` starts a new sentence only when a clause
/// already precedes it and the next word opens that sentence. `U.S. Government` stays together.
/// `We operate in the U.S. Customers adopted it.` splits.
const ABBREVIATIONS: &[&str] = &[
    "capt", "co", "col", "corp", "dr", "eq", "esq", "fig", "gen", "hon", "inc", "jr", "ltd", "mr",
    "mrs", "ms", "no", "plc", "prof", "rev", "sec", "sgt", "sr", "st", "vol",
];
const COMPANY_SUFFIXES: &[&str] = &["co", "corp", "inc", "ltd", "plc"];
const SENTENCE_OPENERS: &[&str] = &[
    "a",
    "an",
    "and",
    "but",
    "he",
    "however",
    "i",
    "it",
    "meanwhile",
    "nor",
    "or",
    "she",
    "so",
    "still",
    "that",
    "the",
    "then",
    "therefore",
    "these",
    "they",
    "this",
    "those",
    "we",
    "yet",
];
const ORDINARY_SUBJECTS: &[&str] = &[
    "cost",
    "costs",
    "demand",
    "earnings",
    "employment",
    "exports",
    "growth",
    "imports",
    "income",
    "inflation",
    "output",
    "price",
    "prices",
    "profit",
    "profits",
    "results",
    "revenue",
    "sales",
];
const FINITE_VERBS: &[&str] = &[
    "are", "be", "been", "can", "could", "did", "do", "does", "fell", "found", "grew", "had",
    "has", "have", "is", "may", "might", "must", "rose", "said", "says", "shall", "should",
    "showed", "shows", "was", "were", "will", "would", "worked", "works",
];
fn abbreviation_before(chars: &[char], period: usize) -> bool {
    let mut start = period;
    while start > 0 && chars[start - 1].is_ascii_alphanumeric() {
        start -= 1;
    }
    let word: String = chars[start..period].iter().collect();
    if word.len() == 1 && word.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return !letter_ends_sentence(chars, period, start);
    }
    let lower = word.to_ascii_lowercase();
    if !ABBREVIATIONS.contains(&lower.as_str()) {
        return false;
    }
    if COMPANY_SUFFIXES.contains(&lower.as_str()) {
        return !company_suffix_ends_sentence(chars, period);
    }
    true
}
fn letter_ends_sentence(chars: &[char], period: usize, letter: usize) -> bool {
    let initialism_end = letter > 0 && chars[letter - 1] == '.';
    if initialism_end {
        return initialism_starts_new_sentence(chars, period, letter);
    }
    // `A. Smith` has no earlier word. `Plan A. Results` does, and the next word opens a sentence.
    multi_letter_word_before(chars, letter) && sentence_opens_after(chars, period)
}
fn initialism_starts_new_sentence(chars: &[char], period: usize, letter: usize) -> bool {
    clause_before_initialism(chars, letter) && initialism_clause_opens(chars, period)
}
fn company_suffix_ends_sentence(chars: &[char], period: usize) -> bool {
    sentence_opens_after(chars, period) || capitalized_finite_clause(chars, period)
}
/// `U.S. Government` stays one name. Another capitalized word plus a finite verb starts a sentence.
fn initialism_clause_opens(chars: &[char], period: usize) -> bool {
    if sentence_opens_after(chars, period) {
        return true;
    }
    let Some((next, _)) = words_after_period(chars, period) else {
        return false;
    };
    if next.eq_ignore_ascii_case("government") {
        return false;
    }
    capitalized_finite_clause(chars, period)
}
fn capitalized_finite_clause(chars: &[char], period: usize) -> bool {
    let Some((next, after)) = words_after_period(chars, period) else {
        return false;
    };
    let Some(first) = next.chars().next() else {
        return false;
    };
    first.is_ascii_uppercase()
        && next.chars().count() > 1
        && after.is_some_and(|word| finite_verb(&word))
}
fn sentence_opens_after(chars: &[char], period: usize) -> bool {
    let Some((next, after)) = words_after_period(chars, period) else {
        return false;
    };
    let Some(first) = next.chars().next() else {
        return false;
    };
    if !first.is_ascii_uppercase() || next.chars().count() == 1 {
        return false;
    }
    let next = next.to_ascii_lowercase();
    if SENTENCE_OPENERS.contains(&next.as_str()) {
        return true;
    }
    ORDINARY_SUBJECTS.contains(&next.as_str()) && after.is_some_and(|word| finite_verb(&word))
}
fn multi_letter_word_before(chars: &[char], letter: usize) -> bool {
    if letter == 0 {
        return false;
    }
    let mut index = letter - 1;
    while chars[index].is_whitespace() {
        if index == 0 {
            return false;
        }
        index -= 1;
    }
    let end = index + 1;
    while index > 0 && chars[index - 1].is_ascii_alphanumeric() {
        index -= 1;
    }
    chars[index..end]
        .iter()
        .filter(|ch| ch.is_ascii_alphabetic())
        .count()
        >= 2
}
fn clause_before_initialism(chars: &[char], letter: usize) -> bool {
    let mut index = letter;
    loop {
        if index == 0 {
            return false;
        }
        index -= 1;
        if chars[index] != '.' {
            break;
        }
        if index == 0 {
            return false;
        }
        let before_dot = index - 1;
        let single_letter = chars[before_dot].is_ascii_alphabetic()
            && (before_dot == 0 || !chars[before_dot - 1].is_ascii_alphanumeric());
        if !single_letter {
            break;
        }
        index = before_dot;
    }
    loop {
        if chars[index].is_ascii_lowercase() {
            return true;
        }
        if index == 0 {
            return false;
        }
        index -= 1;
    }
}
fn words_after_period(chars: &[char], period: usize) -> Option<(String, Option<String>)> {
    let mut look = trailing_syntax_end(chars, period + 1);
    while chars.get(look).is_some_and(|ch| ch.is_whitespace()) {
        look += 1;
    }
    while chars
        .get(look)
        .is_some_and(|ch| matches!(ch, '*' | '_' | '`' | '"' | '\'' | '“' | '‘' | '(' | '（'))
    {
        look += 1;
    }
    let (first, mut look) = take_ascii_word(chars, look)?;
    while chars
        .get(look)
        .is_some_and(|ch| ch.is_whitespace() || matches!(ch, '*' | '_' | '`'))
    {
        look += 1;
    }
    Some((first, take_ascii_word(chars, look).map(|(word, _)| word)))
}
fn take_ascii_word(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut end = start;
    while chars.get(end).is_some_and(|ch| ch.is_ascii_alphanumeric()) {
        end += 1;
    }
    (end > start).then(|| (chars[start..end].iter().collect(), end))
}
fn finite_verb(word: &str) -> bool {
    let word = word.to_ascii_lowercase();
    FINITE_VERBS.contains(&word.as_str())
        || (word.len() >= 4
            && word.ends_with("ed")
            && word.chars().all(|ch| ch.is_ascii_alphabetic()))
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
fn quoted_words(text: &str) -> usize {
    let cjk = text
        .chars()
        .filter(|ch| ('\u{3400}'..='\u{9fff}').contains(ch))
        .count();
    if cjk == 0 {
        text.split_whitespace().count()
    } else {
        cjk + text
            .split_whitespace()
            .filter(|word| word.chars().any(|ch| ch.is_ascii_alphanumeric()))
            .count()
    }
}
fn copied_claim_text(sentence: &str) -> String {
    let mut text = normalize_claim(sentence);
    for marker in citation_markers(&text) {
        while let Some(next) = remove_rightmost_marker(&text, &marker) {
            text = normalize_claim(&next);
        }
    }
    normalize_claim(&strip_article_syntax(&text))
}
fn canonical_citation_number(digits: &str) -> Option<usize> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = digits.parse::<usize>().ok()?;
    (format!("{number}") == digits).then_some(number)
}
fn marker_index(marker: &str) -> Option<usize> {
    marker
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .and_then(canonical_citation_number)
        .and_then(|number| number.checked_sub(1))
}
/// Copied marked sentences count toward the same per-URL budget as short quotes.
/// One sentence position is counted once per URL. A later paste of the same words is another position.
/// A short quote already inside any counted sentence for that same URL is not added again.
fn add_copied_sentence_budget(
    report: &Report,
    urls: &[String],
    quotes: &mut BTreeMap<String, usize>,
) {
    let sentences = sentences_with_trailing_markers(&report.conclusion);
    let mut seen: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let mut billed: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (index, sentence) in sentences.iter().enumerate() {
        if strip_article_syntax(sentence).trim().is_empty() {
            continue;
        }
        let markers = citation_markers(sentence);
        if markers.is_empty() {
            continue;
        }
        let copied = copied_claim_text(sentence);
        if copied.is_empty() {
            continue;
        }
        let words = quoted_words(&copied);
        for marker in markers {
            let Some(cite_index) = marker_index(&marker) else {
                continue;
            };
            let Some(url) = urls.get(cite_index) else {
                continue;
            };
            if seen.entry(url.clone()).or_default().insert(index) {
                *quotes.entry(url.clone()).or_default() += words;
                billed.entry(url.clone()).or_default().push(copied.clone());
            }
        }
    }
    for (cite_index, citation) in report.citations.iter().enumerate() {
        let Some(url) = urls.get(cite_index) else {
            continue;
        };
        let quote = normalize_claim(&citation.quote);
        let already_copied = !quote.is_empty()
            && billed.get(url).is_some_and(|sentences| {
                sentences.iter().any(|sentence| sentence.contains(&quote))
            });
        if !already_copied {
            *quotes.entry(url.clone()).or_default() += quoted_words(&citation.quote);
        }
    }
}
/// `count_copied_sentences` is true only for the claim-paragraph checker.
/// `article-integrity@1` counts short quotes alone, as it did before that rule existed.
pub fn check_report(
    report: &Report,
    analysis: &Value,
    accepted: &[ArtifactData],
    count_copied_sentences: bool,
) -> Result<()> {
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
    let mut urls = Vec::with_capacity(report.citations.len());
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
        if count_copied_sentences {
            urls.push(url);
        } else {
            *quotes.entry(url).or_default() += quoted_words(&c.quote);
        }
    }
    if count_copied_sentences {
        add_copied_sentence_budget(report, &urls, &mut quotes);
    }
    if quotes.values().any(|n| *n > 25) {
        return Err(fault("同一来源累计短引文超过二十五词"));
    }
    for part in report.conclusion.split('[').skip(1) {
        let Some((digits, _)) = part.split_once(']') else {
            continue;
        };
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Some(number) = canonical_citation_number(digits) else {
            return Err(fault("正文引用编号悬空"));
        };
        if !(1..=report.citations.len()).contains(&number) {
            return Err(fault("正文引用编号悬空"));
        }
    }
    Ok(())
}
/// Marked sentences must be stated by their cited paragraphs.
/// `article-integrity@1` does not call this. The claim-paragraph rule does.
pub fn check_claim_sentences(report: &Report, accepted: &[ArtifactData]) -> Result<()> {
    let claims = claims_by_marker(&report.conclusion);
    for marker in claims.keys() {
        let Some(digits) = marker
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        else {
            continue;
        };
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let in_range = canonical_citation_number(digits)
            .is_some_and(|number| (1..=report.citations.len()).contains(&number));
        if !in_range {
            return Err(fault("所引段落没有写出带该标记的主张句"));
        }
    }
    let mut cached: BTreeMap<(String, String, usize), (String, Vec<String>)> = BTreeMap::new();
    for (index, citation) in report.citations.iter().enumerate() {
        let marker = format!("[{}]", index + 1);
        let key = (
            citation.artifact_id.clone(),
            citation.source_id.clone(),
            citation.paragraph,
        );
        if !cached.contains_key(&key) {
            let paragraph = cited_paragraph(citation, accepted)?;
            let sentences = split_sentences(&paragraph)
                .into_iter()
                .map(|sentence| sentence.text)
                .collect();
            cached.insert(key.clone(), (paragraph, sentences));
        }
        let (paragraph, sentences) = &cached[&key];
        let marked = claims.get(&marker).map(Vec::as_slice).unwrap_or(&[]);
        if !paragraph_matches(marked, &marker, &citation.quote, paragraph, sentences) {
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
                    let count_copied = input.rules.rules.iter().any(|rule| rule.id == "claim_paragraph");
                    let structural = check_report(&report, &targets["analysis"].value, &accepted, count_copied).map_err(|error| error.message);
                    let claim = count_copied.then(|| check_claim_sentences(&report, &accepted).map_err(|error| error.message));
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
