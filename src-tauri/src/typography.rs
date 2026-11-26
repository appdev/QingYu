//! 清理排版：中英文之间加空格、中文语境的标点改成全角，交给 AutoCorrect 处理；
//! 它不管的三条（省略号、中文之间的单个破折号、连续空行）在后面补上。
//! 代码、公式、链接地址和 frontmatter 一律原样保留。

pub(crate) fn clean(markdown: &str) -> String {
    let (frontmatter, body) = split_frontmatter(markdown);
    let corrected = autocorrect::format_for(body, "markdown");
    // AutoCorrect 解析失败时不返回半成品，正文保持原样，只做补充规则。
    let body = if corrected.has_error() {
        body.to_string()
    } else {
        corrected.out
    };
    let mut result = String::with_capacity(markdown.len() + 64);
    result.push_str(frontmatter);
    result.push_str(&supplement(&body));
    result
}

/// 开头的 YAML frontmatter 是元数据（标签、日期等），AutoCorrect 会给里面的值加空格，所以整段跳过。
fn split_frontmatter(markdown: &str) -> (&str, &str) {
    let first_end = markdown.find('\n').map_or(markdown.len(), |at| at + 1);
    if markdown[..first_end].trim_end() != "---" {
        return ("", markdown);
    }
    let mut at = first_end;
    while at < markdown.len() {
        let end = markdown[at..]
            .find('\n')
            .map_or(markdown.len(), |offset| at + offset + 1);
        let line = markdown[at..end].trim_end();
        if line == "---" || line == "..." {
            return (&markdown[..end], &markdown[end..]);
        }
        at = end;
    }
    // 没有收尾的 `---` 不是 frontmatter。
    ("", markdown)
}

fn supplement(markdown: &str) -> String {
    let mut output: Vec<String> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut in_math = false;
    let mut blank_run = 0;
    let mut after_indented_code = false;

    for line in markdown.split('\n') {
        let trimmed = line.trim();
        let content = unquoted(trimmed);
        if let Some((marker, length)) = fence {
            if closes_fence(content, marker, length) {
                fence = None;
            }
            output.push(line.to_string());
            blank_run = 0;
            continue;
        }
        if in_math {
            if content.matches("$$").count() % 2 == 1 {
                in_math = false;
            }
            output.push(line.to_string());
            blank_run = 0;
            continue;
        }
        if content.starts_with("$$") && content.matches("$$").count() % 2 == 1 {
            in_math = true;
            output.push(line.to_string());
            blank_run = 0;
            after_indented_code = false;
            continue;
        }
        if let Some(opening) = opening_fence(content) {
            fence = Some(opening);
            output.push(line.to_string());
            blank_run = 0;
            after_indented_code = false;
            continue;
        }
        if line.starts_with("    ") || line.starts_with('\t') {
            // 缩进代码块，或者缩进很深的列表正文。宁可少改，不动它。
            output.push(line.to_string());
            blank_run = 0;
            after_indented_code = true;
            continue;
        }
        if trimmed.is_empty() {
            // 缩进代码块中间的空行是代码的一部分。
            if after_indented_code || blank_run == 0 {
                output.push(line.to_string());
            }
            blank_run += 1;
            continue;
        }
        blank_run = 0;
        after_indented_code = false;
        output.push(clean_line(line));
    }
    output.join("\n")
}

/// 去掉行首的引用标记（`> > `），引用里的代码块围栏也要认得出来。
fn unquoted(trimmed: &str) -> &str {
    let mut rest = trimmed;
    while let Some(inner) = rest.strip_prefix('>') {
        rest = inner.trim_start_matches(' ');
    }
    rest
}

fn opening_fence(content: &str) -> Option<(char, usize)> {
    let marker = content.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let length = content.chars().take_while(|c| *c == marker).count();
    (length >= 3).then_some((marker, length))
}

fn closes_fence(content: &str, marker: char, length: usize) -> bool {
    let run = content.chars().take_while(|c| *c == marker).count();
    run >= length && content[run * marker.len_utf8()..].trim().is_empty()
}

/// 行内代码和链接地址原样保留，其余部分跑省略号和破折号两条规则。
fn clean_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut result = String::with_capacity(line.len() + 8);
    let mut plain: Vec<char> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some(end) = protected_span(&chars, i) {
            result.push_str(&clean_plain(&plain));
            plain.clear();
            result.extend(&chars[i..end]);
            i = end;
        } else {
            plain.push(chars[i]);
            i += 1;
        }
    }
    result.push_str(&clean_plain(&plain));
    result
}

/// 从 `at` 开始如果是行内代码或 `](地址)`，返回它的结束位置。
fn protected_span(chars: &[char], at: usize) -> Option<usize> {
    match chars[at] {
        '`' => {
            let run = chars[at..].iter().take_while(|c| **c == '`').count();
            let mut i = at + run;
            while i < chars.len() {
                if chars[i] == '`' {
                    let close = chars[i..].iter().take_while(|c| **c == '`').count();
                    if close == run {
                        return Some(i + close);
                    }
                    i += close;
                } else {
                    i += 1;
                }
            }
            None
        }
        ']' if chars.get(at + 1) == Some(&'(') => {
            let mut depth = 0;
            for (offset, c) in chars[at + 1..].iter().enumerate() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(at + 2 + offset);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        _ => None,
    }
}

fn clean_plain(chars: &[char]) -> String {
    let mut result: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // 中文后面三个及以上的英文句点是省略号：`等等...` → `等等……`
        if c == '.' && result.last().is_some_and(|prev| is_cjk(*prev)) {
            let run = chars[i..].iter().take_while(|c| **c == '.').count();
            if run >= 3 {
                result.extend(['…', '…']);
                i += run;
                continue;
            }
        }
        // 两个中文之间的单个破折号多半是照搬英文写法，改成逗号；成对的「——」是中文破折号，不动。
        if c == '—' {
            let run = chars[i..].iter().take_while(|c| **c == '—').count();
            let prev = result.iter().rev().find(|c| **c != ' ').copied();
            let next = chars[i + run..].iter().find(|c| **c != ' ').copied();
            if run == 1 && prev.is_some_and(is_cjk) && next.is_some_and(is_cjk) {
                while result.last() == Some(&' ') {
                    result.pop();
                }
                result.push('，');
                i += 1;
                while chars.get(i) == Some(&' ') {
                    i += 1;
                }
                continue;
            }
            result.extend(&chars[i..i + run]);
            i += run;
            continue;
        }
        result.push(c);
        i += 1;
    }
    result.into_iter().collect()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF | 0x3040..=0x30FF | 0x31F0..=0x31FF)
}

#[cfg(feature = "app")]
#[tauri::command]
pub(crate) async fn typography_clean(source: String) -> Result<String, String> {
    // 和网页接口的请求体上限一致。
    if source.len() > 4 * 1024 * 1024 {
        return Err("笔记太大，无法清理排版".into());
    }
    // 长笔记要几十毫秒，放到后台线程，不占用窗口事件循环。
    tauri::async_runtime::spawn_blocking(move || clean(&source))
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn spaces_and_punctuation_follow_autocorrect() {
        assert_eq!(
            clean("用Rust写的工具,很好用!版本3.5,时间10:00。\n"),
            "用 Rust 写的工具，很好用！版本 3.5，时间 10:00。\n"
        );
        assert_eq!(clean("ＡＢＣ１２３"), "ABC123");
    }

    #[test]
    fn keeps_frontmatter_code_math_and_links() {
        let source = "---\ntitle: 测试abc\ntags: [AI笔记]\n---\n\n```bash\necho 中文abc,test\n```\n\n$$\na,b \\text{中文}\n$$\n\n链接[[车机ADB]]和[文档](https://example.com/中文path)\n\n    缩进代码abc\n";
        let cleaned = clean(source);
        assert!(cleaned.starts_with("---\ntitle: 测试abc\ntags: [AI笔记]\n---\n"));
        assert!(cleaned.contains("echo 中文abc,test"));
        assert!(cleaned.contains("a,b \\text{中文}"));
        assert!(cleaned.contains("[[车机ADB]]"));
        assert!(cleaned.contains("(https://example.com/中文path)"));
        assert!(cleaned.contains("    缩进代码abc"));
    }

    #[test]
    fn supplements_ellipsis_dash_and_blank_lines() {
        assert_eq!(clean("等等..."), "等等……");
        assert_eq!(clean("AI 写法 — 很常见"), "AI 写法，很常见");
        assert_eq!(clean("这样做——没问题"), "这样做——没问题");
        assert_eq!(clean("version 1.0 — beta"), "version 1.0 — beta");
        assert_eq!(clean("第一段\n\n\n\n第二段\n"), "第一段\n\n第二段\n");
        assert!(clean("中文`等等...`中文").contains("`等等...`"));
    }

    #[test]
    fn leaves_blank_lines_inside_code_alone() {
        let source = "```\na\n\n\n\nb\n```\n";
        assert_eq!(clean(source), source);
        let indented = "说明：\n\n    a\n\n\n    b\n";
        assert_eq!(clean(indented), indented);
    }

    #[test]
    fn keeps_hard_breaks_and_line_endings() {
        assert_eq!(clean("硬换行  \n下一行"), "硬换行  \n下一行");
        assert_eq!(clean("没有结尾换行"), "没有结尾换行");
        assert_eq!(clean("中文abc\r\n下一行\r\n"), "中文 abc\r\n下一行\r\n");
    }

    #[test]
    fn already_clean_text_is_unchanged() {
        let source = "# 车机 ADB\n\n1. 查看日志：\n   adb logcat -s Mode:D\n";
        assert_eq!(clean(source), source);
    }
}
