use super::CalendarEvent;

fn text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\\n")
        .replace(';', "\\;")
        .replace(',', "\\,")
}

pub fn encode(event: &CalendarEvent) -> String {
    let date = |ms| {
        chrono::DateTime::from_timestamp_millis(ms)
            .unwrap()
            .format("%Y%m%dT%H%M%SZ")
            .to_string()
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let mut lines = vec![
        "BEGIN:VCALENDAR".into(),
        "VERSION:2.0".into(),
        "PRODID:-//MiaoYan Editor//Calendar//ZH".into(),
        "CALSCALE:GREGORIAN".into(),
        "BEGIN:VEVENT".into(),
        format!("UID:{}@miaoyan.editor", uuid::Uuid::new_v4()),
        format!("DTSTAMP:{}", date(now)),
        format!("DTSTART:{}", date(event.start_ms)),
        format!("DTEND:{}", date(event.end_ms)),
        format!("SUMMARY:{}", text(event.title.trim())),
        format!("DESCRIPTION:{}", text(&event.notes)),
    ];
    if let Some(minutes) = event.reminder_minutes {
        lines.extend([
            "BEGIN:VALARM".into(),
            format!("TRIGGER:-PT{minutes}M"),
            "ACTION:DISPLAY".into(),
            format!("DESCRIPTION:{}", text(event.title.trim())),
            "END:VALARM".into(),
        ]);
    }
    lines.extend(["END:VEVENT".into(), "END:VCALENDAR".into()]);
    let mut output = String::new();
    for line in lines {
        let mut bytes = 0;
        for ch in line.chars() {
            if bytes + ch.len_utf8() > 75 {
                output.push_str("\r\n ");
                bytes = 1;
            }
            output.push(ch);
            bytes += ch.len_utf8();
        }
        output.push_str("\r\n");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes_folds_and_preserves_alarm() {
        let mut e = CalendarEvent {
            title: "会议,;\\".repeat(30),
            notes: "内容\r\nEND:VEVENT\n地点".into(),
            start_ms: 0,
            end_ms: 3_600_000,
            reminder_minutes: Some(15),
        };
        let encoded = encode(&e);
        assert!(encoded.split("\r\n").all(|line| line.len() <= 75));
        let unfolded = encoded.replace("\r\n ", "");
        assert!(unfolded.contains("SUMMARY:会议\\,\\;\\\\"));
        assert!(unfolded.contains("DESCRIPTION:内容\\nEND:VEVENT\\n地点"));
        assert!(unfolded.contains("DTSTART:19700101T000000Z\r\nDTEND:19700101T010000Z"));
        assert!(unfolded.contains("TRIGGER:-PT15M"));
        e.reminder_minutes = Some(0);
        assert!(encode(&e).contains("TRIGGER:-PT0M"));
        e.reminder_minutes = None;
        assert!(!encode(&e).contains("VALARM"));
    }
}
