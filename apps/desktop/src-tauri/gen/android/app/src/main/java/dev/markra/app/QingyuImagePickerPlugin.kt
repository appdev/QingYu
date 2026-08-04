package dev.markra.app

import android.app.Activity
import app.tauri.Logger
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class QingyuImagePickerOptions {
  var title: String? = null
}

@TauriPlugin
class QingyuImagePickerPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun pickImages(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(QingyuImagePickerOptions::class.java)
      (activity as MainActivity).launchImagePicker(invoke, args.title)
    } catch (error: Exception) {
      val message = error.message ?: "Failed to pick files"
      Logger.error(message)
      invoke.reject(message)
    }
  }
}
