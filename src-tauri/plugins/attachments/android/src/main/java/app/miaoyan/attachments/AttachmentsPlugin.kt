package app.miaoyan.attachments

import android.app.Activity
import android.content.ClipData
import android.content.Intent
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import java.io.File

@InvokeArg
class OpenArgs { lateinit var path: String; lateinit var mime: String; var share: Boolean = false }

@InvokeArg
class ShareArgs { lateinit var title: String; lateinit var text: String }

@TauriPlugin
class AttachmentsPlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun shareText(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(ShareArgs::class.java)
            val intent = Intent(Intent.ACTION_SEND).setType("text/plain")
                .putExtra(Intent.EXTRA_SUBJECT, args.title).putExtra(Intent.EXTRA_TEXT, args.text)
            activity.startActivity(Intent.createChooser(intent, "发送副本"))
            invoke.resolve()
        } catch (error: Exception) { invoke.reject("无法打开系统分享") }
    }

    @Command
    fun openFile(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(OpenArgs::class.java)
            val file = File(args.path).canonicalFile
            require(file.toPath().startsWith(File(activity.cacheDir, "attachments").canonicalFile.toPath()) && file.isFile && file.length() <= 64L * 1024 * 1024) { "附件路径无效" }
            val uri = FileProvider.getUriForFile(activity, activity.packageName + ".fileprovider", file)
            val intent = if (args.share) {
                Intent(Intent.ACTION_SEND).setType(args.mime).putExtra(Intent.EXTRA_STREAM, uri)
            } else Intent(Intent.ACTION_VIEW).setDataAndType(uri, args.mime)
            intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            intent.clipData = ClipData.newRawUri(file.name, uri)
            activity.startActivity(Intent.createChooser(intent, file.name))
            invoke.resolve()
        } catch (error: Exception) { invoke.reject("无法打开附件，请安装支持此文件类型的应用") }
    }
}
