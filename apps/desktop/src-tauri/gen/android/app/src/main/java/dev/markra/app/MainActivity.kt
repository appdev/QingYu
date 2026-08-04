package dev.markra.app

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import app.tauri.Logger
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import java.lang.ref.WeakReference

class MainActivity : TauriActivity() {
  private var appWebView: WebView? = null
  private lateinit var imagePickerLauncher: ActivityResultLauncher<Intent>
  private val systemBackCallback = object : OnBackPressedCallback(true) {
    override fun handleOnBackPressed() {
      appWebView?.evaluateJavascript(MOBILE_BACK_SCRIPT, null)
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    currentActivity = WeakReference(this)
    imagePickerLauncher = registerForActivityResult(
      ActivityResultContracts.StartActivityForResult()
    ) { result ->
      completeImagePicker(result)
    }
    super.onCreate(savedInstanceState)
    onBackPressedDispatcher.addCallback(this, systemBackCallback)
  }

  override fun onWebViewCreate(webView: WebView) {
    appWebView = webView
    super.onWebViewCreate(webView)
  }

  override fun onDestroy() {
    appWebView = null
    if (currentActivity?.get() === this) {
      currentActivity = null
    }
    if (isFinishing) {
      imagePickerSession.reject("Image picker cancelled")
    }
    super.onDestroy()
  }

  fun launchImagePicker(invoke: Invoke, title: String?) {
    val settlingInvoke = QingyuImagePickerSettlingInvoke(
      TauriImagePickerInvoke(invoke),
      { wakeCurrentMobilePickerEventLoop() }
    )
    if (!imagePickerSession.begin(settlingInvoke)) {
      return
    }

    val intent = Intent(Intent.ACTION_GET_CONTENT)
    intent.addCategory(Intent.CATEGORY_OPENABLE)
    intent.type = "image/*"
    intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
    title?.trim()?.takeIf { it.isNotEmpty() }?.let {
      intent.putExtra(Intent.EXTRA_TITLE, it)
    }

    try {
      imagePickerLauncher.launch(intent)
    } catch (error: Exception) {
      val message = error.message ?: "Failed to pick files"
      Logger.error(message)
      imagePickerSession.reject(message)
    }
  }

  private fun completeImagePicker(result: ActivityResult) {
    try {
      val uris = when (result.resultCode) {
        Activity.RESULT_OK -> imagePickerUris(result.data)
        Activity.RESULT_CANCELED -> emptyList()
        else -> emptyList()
      }
      if (!imagePickerSession.complete(result.resultCode, uris)) {
        Logger.error("Image picker result arrived without a pending invoke")
      }
    } catch (error: Exception) {
      val message = error.message ?: "Failed to read image pick result"
      Logger.error(message)
      if (!imagePickerSession.reject(message)) {
        Logger.error("Image picker error arrived without a pending invoke")
      }
    }
  }

  private fun imagePickerUris(data: Intent?): List<String> {
    val uris = mutableListOf<String>()
    val clipData = data?.clipData
    if (clipData != null) {
      for (index in 0 until clipData.itemCount) {
        clipData.getItemAt(index).uri?.let { uri: Uri -> uris.add(uri.toString()) }
      }
    } else {
      data?.data?.let { uris.add(it.toString()) }
    }
    return uris
  }

  private fun wakeMobilePickerEventLoop() {
    val webView = appWebView ?: return
    webView.post {
      try {
        webView.evaluateJavascript(MOBILE_PICKER_EVENT_LOOP_WAKE_SCRIPT, null)
      } catch (_: Exception) {
      }
    }
  }

  private class TauriImagePickerInvoke(private val invoke: Invoke) : QingyuImagePickerInvoke {
    override fun resolveImagePickerUris(uris: List<String>) {
      invoke.resolve(imagePickerResponse(uris))
    }

    override fun rejectImagePicker(message: String) {
      invoke.reject(message)
    }

    private fun imagePickerResponse(uris: List<String>): JSObject {
      val response = JSObject()
      response.put("uris", JSArray.from(uris.toTypedArray()))
      return response
    }
  }

  companion object {
    private val imagePickerSession = QingyuImagePickerSession()
    private var currentActivity: WeakReference<MainActivity>? = null

    fun launchImagePickerFromCurrentActivity(invoke: Invoke, title: String?) {
      val activity = currentActivity?.get()
      if (activity == null || activity.isFinishing || activity.isDestroyed) {
        invoke.reject("Image picker unavailable")
        return
      }
      activity.launchImagePicker(invoke, title)
    }

    private fun wakeCurrentMobilePickerEventLoop() {
      currentActivity?.get()?.wakeMobilePickerEventLoop()
    }

    private const val MOBILE_BACK_SCRIPT =
      "window.dispatchEvent(new Event('qingyu://mobile-back-requested'))"
    private const val MOBILE_PICKER_EVENT_LOOP_WAKE_SCRIPT =
      "(function(){var tauri=window.__TAURI_INTERNALS__;" +
        "if(tauri&&typeof tauri.invoke==='function'){" +
        "try{Promise.resolve(tauri.invoke('wake_mobile_picker_event_loop')).catch(function(){});}" +
        "catch(_error){}" +
        "}})()"
  }
}
