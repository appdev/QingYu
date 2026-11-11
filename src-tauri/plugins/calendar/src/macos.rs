use super::CalendarEvent;
use block2::RcBlock;
use objc2::{runtime::Bool, sel};
use objc2_event_kit::{EKAlarm, EKEntityType, EKEvent, EKEventStore, EKSpan};
use objc2_foundation::{NSDate, NSError, NSObjectProtocol, NSString};

pub fn create(input: CalendarEvent) -> Result<String, String> {
    // The store and events stay on this worker; only the permission result crosses threads.
    unsafe {
        let store = EKEventStore::new();
        let (send, receive) = std::sync::mpsc::channel();
        let completion = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            let _ = send.send(granted.as_bool());
        });
        if store.respondsToSelector(sel!(requestWriteOnlyAccessToEventsWithCompletion:)) {
            store.requestWriteOnlyAccessToEventsWithCompletion(RcBlock::as_ptr(&completion));
        } else {
            #[allow(deprecated)]
            store.requestAccessToEntityType_completion(
                EKEntityType::Event,
                RcBlock::as_ptr(&completion),
            );
        }
        if !receive.recv().map_err(|_| "无法获取日历权限")? {
            return Err("未获日历权限，请在系统设置 → 隐私与安全性 → 日历中允许访问".into());
        }
        let calendar = store
            .defaultCalendarForNewEvents()
            .ok_or("没有可写入的默认日历，请先在系统日历中添加日历账户")?;
        let event = EKEvent::eventWithEventStore(&store);
        event.setCalendar(Some(&calendar));
        event.setTitle(Some(&NSString::from_str(input.title.trim())));
        event.setNotes(Some(&NSString::from_str(&input.notes)));
        event.setStartDate(Some(&NSDate::dateWithTimeIntervalSince1970(
            input.start_ms as f64 / 1000.0,
        )));
        event.setEndDate(Some(&NSDate::dateWithTimeIntervalSince1970(
            input.end_ms as f64 / 1000.0,
        )));
        if let Some(minutes) = input.reminder_minutes {
            event.addAlarm(&EKAlarm::alarmWithRelativeOffset(-(minutes as f64) * 60.0));
        }
        store
            .saveEvent_span_error(&event, EKSpan::ThisEvent)
            .map_err(|error| format!("日程保存失败：{}", error.localizedDescription()))?;
        Ok(format!("日程已写入日历「{}」", calendar.title()))
    }
}
