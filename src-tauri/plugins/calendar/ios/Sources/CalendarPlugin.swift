import EventKit
import Tauri

private struct EventArgs: Decodable {
    let title: String
    let notes: String
    let startMs: Int64
    let endMs: Int64
    let reminderMinutes: Int?
}

class CalendarPlugin: Plugin {
    @objc func createEvent(_ invoke: Invoke) {
        do {
            let args = try invoke.parseArgs(EventArgs.self)
            guard !args.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                  args.title.unicodeScalars.count <= 200,
                  !args.title.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }),
                  args.notes.unicodeScalars.count <= 10000, !args.notes.contains("\0"),
                  args.startMs >= 0, args.endMs > args.startMs, args.endMs <= 253402300799000,
                  args.reminderMinutes.map({ (0...40320).contains($0) }) ?? true else {
                invoke.reject("日程标题、时间或提醒无效"); return
            }
            let store = EKEventStore()
            let save: (Bool, Error?) -> Void = { granted, error in
                guard granted else {
                    invoke.reject("未获日历权限，请在系统设置中允许访问日历后重试"); return
                }
                guard let calendar = store.defaultCalendarForNewEvents else {
                    invoke.reject("没有可写入的默认日历，请先在系统日历中添加日历账户"); return
                }
                let event = EKEvent(eventStore: store)
                event.calendar = calendar
                event.title = args.title.trimmingCharacters(in: .whitespacesAndNewlines)
                event.notes = args.notes
                event.startDate = Date(timeIntervalSince1970: Double(args.startMs) / 1000)
                event.endDate = Date(timeIntervalSince1970: Double(args.endMs) / 1000)
                if let minutes = args.reminderMinutes { event.addAlarm(EKAlarm(relativeOffset: -Double(minutes) * 60)) }
                do {
                    try store.save(event, span: .thisEvent)
                    invoke.resolve(["message": "日程已写入系统日历"])
                } catch { invoke.reject("创建日程失败：\(error.localizedDescription)") }
            }
            if #available(iOS 17.0, *) { store.requestWriteOnlyAccessToEvents(completion: save) }
            else { store.requestAccess(to: .event, completion: save) }
        } catch { invoke.reject("日程参数无效") }
    }
}

@_cdecl("init_plugin_calendar")
public func initPlugin() -> Plugin { CalendarPlugin() }
