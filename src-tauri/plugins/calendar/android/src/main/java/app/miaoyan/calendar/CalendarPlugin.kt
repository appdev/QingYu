package app.miaoyan.calendar

import android.Manifest
import android.app.Activity
import android.content.ContentProviderOperation
import android.provider.CalendarContract
import android.provider.CalendarContract.Calendars
import android.provider.CalendarContract.Events
import android.provider.CalendarContract.Reminders
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.util.TimeZone

@InvokeArg
class EventArgs {
    lateinit var title: String
    var notes: String = ""
    var startMs: Long = 0
    var endMs: Long = 0
    var reminderMinutes: Int? = null
}

@TauriPlugin(permissions = [Permission(alias = "calendar", strings = [Manifest.permission.READ_CALENDAR, Manifest.permission.WRITE_CALENDAR])])
class CalendarPlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun createEvent(invoke: Invoke) {
        if (getPermissionState("calendar") != PermissionState.GRANTED) {
            requestPermissionForAlias("calendar", invoke, "calendarPermission")
        } else save(invoke)
    }

    @PermissionCallback
    fun calendarPermission(invoke: Invoke) {
        if (getPermissionState("calendar") == PermissionState.GRANTED) save(invoke)
        else invoke.reject("未获日历权限，请在系统设置中允许访问日历后重试")
    }

    private fun save(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(EventArgs::class.java)
            require(args.title.isNotBlank() && args.title.codePointCount(0, args.title.length) <= 200 && args.title.none { it.isISOControl() } && args.notes.codePointCount(0, args.notes.length) <= 10000 && '\u0000' !in args.notes) { "日程标题或备注无效" }
            require(args.startMs >= 0 && args.endMs > args.startMs && args.endMs <= 253402300799000L) { "日程起止时间无效" }
            require(args.reminderMinutes == null || args.reminderMinutes!! in 0..40320) { "提醒时间无效" }
            val resolver = activity.contentResolver
            var calendarId: Long? = null
            var calendarName = "系统日历"
            resolver.query(Calendars.CONTENT_URI,
                arrayOf(Calendars._ID, Calendars.CALENDAR_DISPLAY_NAME, Calendars.IS_PRIMARY, Calendars.MAX_REMINDERS, Calendars.ALLOWED_REMINDERS),
                "${Calendars.CALENDAR_ACCESS_LEVEL} >= ? AND ${Calendars.VISIBLE} = 1",
                arrayOf(Calendars.CAL_ACCESS_CONTRIBUTOR.toString()), "${Calendars.IS_PRIMARY} DESC, ${Calendars._ID} ASC")?.use { cursor ->
                while (cursor.moveToNext()) {
                    if (args.reminderMinutes != null && (cursor.getInt(3) < 1 || cursor.getString(4)?.split(',')?.contains(CalendarContract.Reminders.METHOD_ALERT.toString()) != true)) continue
                    calendarId = cursor.getLong(0)
                    calendarName = cursor.getString(1) ?: calendarName
                    break
                }
            }
            val id = calendarId ?: throw IllegalStateException("没有支持此提醒设置的可写日历，请先在系统日历中添加账户或本地日历")
            val operations = arrayListOf(ContentProviderOperation.newInsert(Events.CONTENT_URI)
                .withValue(Events.CALENDAR_ID, id).withValue(Events.TITLE, args.title.trim())
                .withValue(Events.DESCRIPTION, args.notes).withValue(Events.DTSTART, args.startMs).withValue(Events.DTEND, args.endMs)
                .withValue(Events.EVENT_TIMEZONE, TimeZone.getDefault().id)
                .withValue(Events.HAS_ALARM, if (args.reminderMinutes == null) 0 else 1).build())
            args.reminderMinutes?.let { minutes ->
                operations.add(ContentProviderOperation.newInsert(Reminders.CONTENT_URI)
                    .withValueBackReference(Reminders.EVENT_ID, 0).withValue(Reminders.MINUTES, minutes)
                    .withValue(Reminders.METHOD, Reminders.METHOD_ALERT).build())
            }
            resolver.applyBatch(CalendarContract.AUTHORITY, operations)
            invoke.resolve(JSObject().put("message", "日程已写入日历「$calendarName」"))
        } catch (error: Exception) {
            invoke.reject("创建日程失败：${error.message ?: "请检查系统日历设置"}")
        }
    }
}
